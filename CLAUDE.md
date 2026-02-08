# CLAUDE.md

## Project Overview

COSCUP Newsletter — 電子報訂閱管理系統（Rust 全新重寫），取代舊版 Python/Flask + MongoDB 系統。

## Tech Stack

- **Language**: Rust
- **Web Framework**: Axum 0.8
- **Template Engine**: Tera (SSR)
- **Database**: PostgreSQL (sqlx, auto-migration on startup)
- **Email**: SMTP via `lettre`（相容 AWS SES SMTP、Mailgun 等任何 SMTP 服務）
- **Captcha**: Cloudflare Turnstile
- **Deployment**: Docker Compose

## Commands

```bash
# 完整驗證（每次改動必須通過）
cargo fmt --check && cargo clippy -- -D warnings && cargo test

# 本地開發環境（PostgreSQL + MailHog）
docker compose -f docker-compose.dev.yml up -d

# 啟動服務
cargo run
```

## Code Quality Requirements

- **零 warning 政策**: `cargo clippy -- -D warnings` 必須通過，所有 clippy lint 視為 error
- **格式化**: `cargo fmt --check` 必須通過
- **測試**: `cargo test` 所有測試必須通過
- **unsafe 禁止**: `unsafe_code = "forbid"`
- **Clippy pedantic**: 已啟用，部分過嚴的 lint 已在 `Cargo.toml` 中放寬（`module_name_repetitions`、`missing_errors_doc`、`missing_panics_doc`）

## Development Methodology

- **TDD (Test-Driven Development)**: 遵循 Red-Green-Refactor 流程
  1. **Red** — 先寫失敗的測試，定義預期行為
  2. **Green** — 寫最少的程式碼讓測試通過
  3. **Refactor** — 重構，確保測試仍通過
- 每個功能模組在 `#[cfg(test)] mod tests` 中寫單元測試
- 外部依賴（email、captcha）透過 trait 抽象，測試中用 mock 實作
- 每次改動後執行完整驗證：`cargo fmt --check && cargo clippy -- -D warnings && cargo test`

## Architecture Patterns

- **Error Handling**: 自定義 `AppError` enum 實作 `IntoResponse`，統一錯誤回傳，隱藏內部細節
- **State**: `AppState` 包含 DB pool、config、tera、email service、captcha verifier，透過 Axum `State` 注入
- **Trait 抽象**: `EmailService` 和 `CaptchaVerifier` 為 trait，方便測試中用 mock 實作
- **Security**: SHA256 for admin_link, HMAC-SHA256 for openhash, `subtle` crate for constant-time comparison
- **Legacy 相容**: 舊使用者的 `admin_link` 保留在 `legacy_admin_link` 欄位，驗證時優先比對

## Project Structure

```
src/
├── main.rs           # 啟動、路由註冊、graceful shutdown
├── config.rs         # 環境變數讀取
├── error.rs          # 統一錯誤處理
├── db.rs             # PostgreSQL 連線池 + migration
├── security.rs       # 雜湊、HMAC、token 產生/驗證
├── email.rs          # SMTP 發信（trait 抽象）
├── captcha.rs        # Cloudflare Turnstile 驗證（trait 抽象）
├── csv_handler.rs    # CSV 匯入/匯出
├── routes/           # 路由 handlers
└── templates/        # Tera HTML 模板
```

## Adding Dependencies

- 新增套件前，必須確認其 License 與本專案 AGPL-3.0 相容
- 相容的 License：MIT、Apache-2.0、BSD-2-Clause、BSD-3-Clause、ISC、Zlib、MPL-2.0
- 不相容的 License：GPL-2.0-only（非 "or later"）、proprietary、SSPL
- 可用 `cargo license` 或查看 crate 的 crates.io 頁面確認

## Known Gotchas

- Axum 0.8 路徑參數用 `{param}` 語法（非 `:param`）
- Tera strict mode：模板中所有變數必須在 Context 中定義，或用 `| default(value="")` 防禦
- `axum-extra` 需要 `cookie` feature 才能用 session cookie
- Cookie `max_age` 需要 `time` crate（非 `std::time`）
- Axum 需要 `multipart` feature 才能用 `Multipart` extractor

## License

AGPL-3.0，與原專案 [COSCUP/subscribe](https://github.com/COSCUP/subscribe) 保持一致。
