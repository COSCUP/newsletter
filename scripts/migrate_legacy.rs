use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut csv_path = String::new();
    let mut database_url = String::new();
    let mut dry_run = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => {
                i += 1;
                if i < args.len() {
                    csv_path.clone_from(&args[i]);
                }
            }
            "--database-url" => {
                i += 1;
                if i < args.len() {
                    database_url.clone_from(&args[i]);
                }
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--help" | "-h" => {
                println!("Usage: migrate-legacy --csv <path> --database-url <url> [--dry-run]");
                process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                process::exit(1);
            }
        }
        i += 1;
    }

    if csv_path.is_empty() {
        eprintln!("Error: --csv is required");
        process::exit(1);
    }

    if database_url.is_empty() {
        database_url = env::var("DATABASE_URL").unwrap_or_default();
    }

    if database_url.is_empty() && !dry_run {
        eprintln!("Error: --database-url or DATABASE_URL is required");
        process::exit(1);
    }

    let csv_data = match fs::read_to_string(&csv_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading CSV file: {e}");
            process::exit(1);
        }
    };

    let mut reader = csv::Reader::from_reader(csv_data.as_bytes());
    let mut count = 0;
    let mut errors = 0;

    for result in reader.records() {
        match result {
            Ok(record) => {
                let _email = record.get(2).unwrap_or("").trim();
                let name = record.get(1).unwrap_or("").trim();
                let clean_mail = record.get(3).unwrap_or("").trim();
                let status = record.get(4).unwrap_or("0");
                let verified_email = record.get(5).unwrap_or("0");
                let admin_link = record.get(6).unwrap_or("").trim();
                let ucode = record.get(7).unwrap_or("").trim();

                if clean_mail.is_empty() {
                    eprintln!("Skipping record with empty email");
                    errors += 1;
                    continue;
                }

                if dry_run {
                    println!(
                        "[DRY RUN] Would import: email={clean_mail}, name={name}, ucode={ucode}, status={status}, legacy_admin_link={admin_link}"
                    );
                } else {
                    println!("Importing: email={clean_mail}, name={name}, ucode={ucode}");
                    // In a real implementation, we'd use sqlx here.
                    // This binary is a simplified CLI that would need tokio runtime for DB access.
                    // For now, output SQL statements that can be piped to psql.
                    let secret_code = generate_hex(32);
                    let status_bool = status == "1";
                    let verified_bool = verified_email == "1";
                    println!(
                        "INSERT INTO subscribers (email, name, secret_code, ucode, legacy_admin_link, status, verified_email, subscription_source) \
                         VALUES ('{clean_mail}', '{}', '{secret_code}', '{ucode}', '{admin_link}', {status_bool}, {verified_bool}, 'legacy') \
                         ON CONFLICT (email) DO NOTHING;",
                        name.replace('\'', "''")
                    );
                }
                count += 1;
            }
            Err(e) => {
                eprintln!("Error parsing record: {e}");
                errors += 1;
            }
        }
    }

    println!("\nProcessed: {count}, Errors: {errors}");
    if dry_run {
        println!("(Dry run - no changes made)");
    }
}

fn generate_hex(bytes: usize) -> String {
    use std::fmt::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    // Simple hex generation for CLI tool (not crypto-grade, just unique enough for migration)
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut result = String::with_capacity(bytes * 2);
    for i in 0..bytes {
        #[allow(clippy::cast_possible_truncation)]
        let byte = ((seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(i as u128))
            >> 8) as u8;
        let _ = write!(result, "{byte:02x}");
    }
    result
}
