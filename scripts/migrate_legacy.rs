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

    let records = parse_import_csv(&csv_data);
    let mut count = 0;
    let mut errors = 0;

    for record in &records {
        if record.email.is_empty() {
            eprintln!("Skipping record with empty email");
            errors += 1;
            continue;
        }

        if dry_run {
            println!(
                "[DRY RUN] Would import: email={}, name={}, ucode={}, status={}, legacy_admin_link={}",
                record.email, record.name, record.ucode, record.status, record.legacy_admin_link
            );
        } else {
            println!(
                "Importing: email={}, name={}, ucode={}",
                record.email, record.name, record.ucode
            );
            let secret_code = generate_hex(32);
            println!(
                "INSERT INTO subscribers (email, name, secret_code, ucode, legacy_admin_link, status, verified_email, subscription_source) \
                 VALUES ('{}', '{}', '{secret_code}', '{}', '{}', {}, {}, 'legacy') \
                 ON CONFLICT (email) DO NOTHING;",
                record.email,
                record.name.replace('\'', "''"),
                record.ucode,
                record.legacy_admin_link,
                record.status,
                record.verified_email,
            );
        }
        count += 1;
    }

    println!("\nProcessed: {count}, Errors: {errors}");
    if dry_run {
        println!("(Dry run - no changes made)");
    }
}

struct ImportRecord {
    email: String,
    name: String,
    ucode: String,
    status: bool,
    verified_email: bool,
    legacy_admin_link: String,
}

fn parse_import_csv(data: &str) -> Vec<ImportRecord> {
    let first_line = data.lines().next().unwrap_or("");
    let headers: Vec<&str> = first_line.split(',').map(str::trim).collect();

    let mut reader = csv::Reader::from_reader(data.as_bytes());
    let mut records = Vec::new();

    if headers.contains(&"uid") && headers.contains(&"created_at") {
        // V2 format: uid,mail,name,created_at
        for result in reader.records() {
            match result {
                Ok(row) => {
                    records.push(ImportRecord {
                        email: row.get(1).unwrap_or("").trim().to_string(),
                        name: row.get(2).unwrap_or("").trim().to_string(),
                        ucode: row.get(0).unwrap_or("").trim().to_string(),
                        status: true,
                        verified_email: true,
                        legacy_admin_link: String::new(),
                    });
                }
                Err(e) => eprintln!("Error parsing record: {e}"),
            }
        }
    } else if headers.contains(&"_id") && headers.contains(&"clean_mail") {
        // V1 format: _id,name,mail,clean_mail,status,verified_email,admin_link,ucode,...
        for result in reader.records() {
            match result {
                Ok(row) => {
                    records.push(ImportRecord {
                        email: row.get(3).unwrap_or("").trim().to_string(),
                        name: row.get(1).unwrap_or("").trim().to_string(),
                        ucode: row.get(7).unwrap_or("").trim().to_string(),
                        status: row.get(4).unwrap_or("0") == "1",
                        verified_email: row.get(5).unwrap_or("0") == "1",
                        legacy_admin_link: row.get(6).unwrap_or("").trim().to_string(),
                    });
                }
                Err(e) => eprintln!("Error parsing record: {e}"),
            }
        }
    } else {
        eprintln!("Unrecognized CSV format: expected headers with '_id,clean_mail' (v1) or 'uid,created_at' (v2)");
    }

    records
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
