# COSCUP Newsletter

COSCUP 電子報訂閱管理系統，提供訂閱、Email 驗證（Double Opt-in）、自助管理、開信追蹤及管理後台功能。

## 技術棧

| 層級 | 技術 |
|------|------|
| Language | Rust |
| Web Framework | Axum 0.8 |
| Template Engine | Tera (SSR) |
| Database | PostgreSQL |
| Email | SMTP（相容 AWS SES SMTP） |
| Captcha | Cloudflare Turnstile |
| Deployment | Docker Compose |

## 快速開始

### 前置需求

- Rust 1.93+
- PostgreSQL
- SMTP 服務（如 AWS SES SMTP、Mailgun、自建 SMTP 等）
- Cloudflare Turnstile 帳號

### 1. 設定環境變數

```bash
cp .env.example .env
```

編輯 `.env`，填入實際設定值：

```env
DATABASE_URL=postgres://user:password@localhost:5432/coscup_newsletter
HOST=0.0.0.0
PORT=8080
BASE_URL=http://localhost:8080

# 允許登入 Admin 後台的 Email（逗號分隔）
ADMIN_EMAILS=admin@coscup.org

# Cloudflare Turnstile
TURNSTILE_SECRET=your-turnstile-secret
TURNSTILE_SITEKEY=your-turnstile-sitekey

# SMTP
SMTP_HOST=localhost
SMTP_PORT=1025
SMTP_TLS=false
SMTP_FROM_EMAIL=newsletter@coscup.org
# SMTP_USERNAME=your-smtp-username    # 可選
# SMTP_PASSWORD=your-smtp-password    # 可選
```

若使用 AWS SES SMTP，設定範例：

```env
SMTP_HOST=email-smtp.ap-northeast-1.amazonaws.com
SMTP_PORT=587
SMTP_TLS=true
SMTP_USERNAME=your-ses-smtp-username
SMTP_PASSWORD=your-ses-smtp-password
SMTP_FROM_EMAIL=newsletter@coscup.org
```

### 2. 啟動 PostgreSQL（Docker）

專案提供 `docker-compose.dev.yml` 方便本地開發：

```bash
docker compose -f docker-compose.dev.yml up -d
```

這會啟動：
- **PostgreSQL 16**（`localhost:5432`，帳號/密碼皆為 `coscup`）
- **MailHog**（SMTP: `localhost:1025`，Web UI: `http://localhost:8025`）— 攔截所有寄出的 Email，方便本地測試

Migration 會在程式啟動時自動執行（`migrations/001_initial.sql`），無需手動建表。

停止 / 清除資料庫：

```bash
docker compose -f docker-compose.dev.yml down       # 停止（保留資料）
docker compose -f docker-compose.dev.yml down -v     # 停止並清除資料
```

> 若已有外部 PostgreSQL，跳過此步，直接修改 `.env` 中的 `DATABASE_URL`。

### 3. 本地開發

```bash
cargo run
```

服務啟動後可存取 `http://localhost:8080`。

`.env` 中的 Turnstile 測試 key（`1x0000...0AA`）會讓 captcha 永遠通過，方便本地測試。

### 4. Docker 部署

```bash
docker compose up -d --build
```

PostgreSQL 需由外部提供，透過 `DATABASE_URL` 連線。

## 路由總覽

### 公開頁面

| Method | Path | 說明 |
|--------|------|------|
| GET | `/` | 訂閱表單 |
| POST | `/api/subscribe` | 提交訂閱（含 Cloudflare Turnstile 驗證） |
| GET | `/verify/{token}` | Email 驗證連結 |
| GET | `/manage/{admin_link}` | 訂閱者自助管理頁面 |
| POST | `/manage/{admin_link}/update` | 更新名稱 |
| POST | `/manage/{admin_link}/unsubscribe` | 取消訂閱 |
| GET | `/track/open?ucode=&topic=&hash=` | 開信追蹤（回傳 1x1 透明 PNG） |
| GET | `/track/click?ucode=&topic=&hash=&url=` | 點擊追蹤（302 重導向） |
| GET | `/health` | Health check |

### Admin 後台（需登入）

| Method | Path | 說明 |
|--------|------|------|
| GET | `/admin/login` | 登入頁 |
| POST | `/admin/login` | 發送 Magic Link |
| GET | `/admin/auth/{token}` | Magic Link 驗證 + 建立 Session |
| GET | `/admin` | Dashboard（總覽數據） |
| GET | `/admin/subscribers` | 訂閱者列表（分頁、搜尋） |
| POST | `/admin/subscribers/import` | CSV 匯入 |
| GET | `/admin/subscribers/export` | CSV 匯出 |
| POST | `/admin/subscribers/{id}/toggle` | 切換訂閱狀態 |
| POST | `/admin/subscribers/{id}/resend` | 重發驗證信 |
| GET | `/admin/stats` | 開信/點擊統計 |
| POST | `/admin/logout` | 登出 |

## 舊資料遷移

系統支援從舊版（Python/Flask + MongoDB）匯出的 CSV 匯入，保留舊使用者的 `admin_link` 確保管理連結不失效。

### 透過 Admin 後台匯入

登入 Admin 後台 → 訂閱者 → 選擇 CSV 檔案 → 匯入。

### 透過 CLI 匯入

```bash
# 先行驗證（不寫入資料庫）
cargo run --bin migrate-legacy -- --csv example.csv --dry-run

# 產生 SQL 語句
cargo run --bin migrate-legacy -- --csv example.csv --database-url $DATABASE_URL
```

CSV 格式需符合舊系統匯出格式：

```
_id,name,mail,clean_mail,status,verified_email,admin_link,ucode,args,openhash
```

## 安全機制

- **Secret Code**: 每位訂閱者有獨立的 32-byte 隨機密鑰
- **Admin Link**: `SHA256(secret_code || email)`，作為永久管理連結
- **Openhash**: `HMAC-SHA256(secret_code, "ucode:topic")`，防止追蹤連結被竄改
- **Legacy 相容**: 舊使用者的 `admin_link` 直接比對 `legacy_admin_link` 欄位
- **Admin 認證**: Magic Link（15 分鐘有效），Session Cookie（24 小時有效，HttpOnly）
- **Constant-time 比對**: 使用 `subtle` crate 防止 timing attack

## 開發

### 專案結構

```
src/
├── main.rs           # 啟動、路由註冊、graceful shutdown
├── config.rs         # 環境變數讀取
├── error.rs          # 統一錯誤處理（AppError → HTTP Response）
├── db.rs             # PostgreSQL 連線池 + migration
├── security.rs       # 雜湊、HMAC、token 產生/驗證
├── email.rs          # SMTP 發信（trait 抽象，相容任何 SMTP 服務）
├── captcha.rs        # Cloudflare Turnstile 驗證（trait 抽象）
├── csv_handler.rs    # CSV 匯入/匯出
├── routes/
│   ├── subscribe.rs  # 訂閱 + Email 驗證
│   ├── manage.rs     # 自助管理
│   ├── tracking.rs   # 開信/點擊追蹤
│   └── admin.rs      # 管理後台
└── templates/        # Tera HTML 模板
```

### 測試

```bash
cargo test
```

目前包含 24 個單元測試，涵蓋：
- Security（secret code、admin link、openhash、constant-time 比對）
- Config（admin email 驗證）
- Error（HTTP status mapping）
- CSV（legacy 解析、匯出格式）
- Email / Captcha（mock 實作）

### Lint

```bash
cargo fmt --check     # 格式檢查
cargo clippy -- -D warnings  # Clippy（零 warning 政策）
```

### 完整驗證

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## 致謝

本專案基於 [COSCUP/subscribe](https://github.com/COSCUP/subscribe) 的功能設計與資料結構進行全新重寫。感謝原專案作者 [@toomore](https://github.com/toomore) 與 [@orertrr](https://github.com/orertrr) 建立了完整的電子報訂閱系統，為本專案提供了寶貴的參考基礎。

## License

本專案採用 [AGPL-3.0](LICENSE) 授權，與原專案 [COSCUP/subscribe](https://github.com/COSCUP/subscribe) 保持一致。
