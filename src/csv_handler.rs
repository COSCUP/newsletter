use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LegacyCsvRecord {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub mail: String,
    #[serde(rename = "clean_mail")]
    pub clean_mail: String,
    pub status: String,
    pub verified_email: String,
    pub admin_link: String,
    pub ucode: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub openhash: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LegacyV2CsvRecord {
    pub uid: String,
    pub mail: String,
    pub name: String,
    pub created_at: String,
}

/// Normalized import record from any legacy CSV format.
#[derive(Debug, PartialEq, Eq)]
pub struct ImportRecord {
    pub email: String,
    pub name: String,
    pub ucode: String,
    pub status: bool,
    pub verified_email: bool,
    pub legacy_admin_link: String,
}

#[derive(Debug, Serialize)]
pub struct ExportCsvRecord {
    pub email: String,
    pub name: String,
    pub ucode: String,
    pub status: bool,
    pub admin_link: String,
    pub openhash: String,
}

pub fn parse_legacy_csv(data: &str) -> Result<Vec<LegacyCsvRecord>, csv::Error> {
    let mut reader = csv::Reader::from_reader(data.as_bytes());
    reader.deserialize().collect()
}

fn parse_legacy_v2_csv(data: &str) -> Result<Vec<LegacyV2CsvRecord>, csv::Error> {
    let mut reader = csv::Reader::from_reader(data.as_bytes());
    reader.deserialize().collect()
}

/// Auto-detect CSV format by headers and parse into unified `ImportRecord`s.
pub fn parse_import_csv(data: &str) -> Result<Vec<ImportRecord>, csv::Error> {
    let first_line = data.lines().next().unwrap_or("");
    let headers: Vec<&str> = first_line.split(',').map(str::trim).collect();

    if headers.contains(&"uid") && headers.contains(&"created_at") {
        // V2 format: uid,mail,name,created_at
        let records = parse_legacy_v2_csv(data)?;
        Ok(records
            .into_iter()
            .map(|r| ImportRecord {
                email: r.mail,
                name: r.name,
                ucode: r.uid,
                status: true,
                verified_email: true,
                legacy_admin_link: String::new(),
            })
            .collect())
    } else if headers.contains(&"_id") && headers.contains(&"clean_mail") {
        // V1 format: _id,name,mail,clean_mail,status,verified_email,admin_link,ucode,...
        let records = parse_legacy_csv(data)?;
        Ok(records
            .into_iter()
            .map(|r| ImportRecord {
                email: r.clean_mail,
                name: r.name,
                ucode: r.ucode,
                status: r.status == "1",
                verified_email: r.verified_email == "1",
                legacy_admin_link: r.admin_link,
            })
            .collect())
    } else {
        Err(csv::Error::from(std::io::Error::other(
            "Unrecognized CSV format: expected headers with '_id,clean_mail' (v1) or 'uid,created_at' (v2)",
        )))
    }
}

pub fn write_export_csv(records: &[ExportCsvRecord]) -> Result<String, csv::Error> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for record in records {
        writer.serialize(record)?;
    }
    let data = writer
        .into_inner()
        .map_err(|e| csv::Error::from(std::io::Error::other(e.to_string())))?;
    Ok(String::from_utf8_lossy(&data).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_legacy_csv() {
        let csv_data = "_id,name,mail,clean_mail,status,verified_email,admin_link,ucode,args,openhash\nyoyo930021@gmail.com,yoyo930021,yoyo930021@gmail.com,yoyo930021@gmail.com,1,0,a8c11d7b8171fddb207e8b321589efbaac388ccc6089fb4859146c1f659c9040,b3514a49,t=eos,7c4897996408bcfb803c59805dd17b061262092e6c86f933eea3306bc43eb5d5";
        let records = parse_legacy_csv(csv_data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].mail, "yoyo930021@gmail.com");
        assert_eq!(records[0].name, "yoyo930021");
        assert_eq!(records[0].ucode, "b3514a49");
        assert_eq!(records[0].status, "1");
        assert_eq!(
            records[0].admin_link,
            "a8c11d7b8171fddb207e8b321589efbaac388ccc6089fb4859146c1f659c9040"
        );
    }

    #[test]
    fn test_parse_legacy_csv_empty() {
        let csv_data =
            "_id,name,mail,clean_mail,status,verified_email,admin_link,ucode,args,openhash\n";
        let records = parse_legacy_csv(csv_data).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_parse_import_csv_v1() {
        let csv_data = "_id,name,mail,clean_mail,status,verified_email,admin_link,ucode,args,openhash\nyoyo930021@gmail.com,yoyo930021,yoyo930021@gmail.com,yoyo930021@gmail.com,1,0,a8c11d7b,b3514a49,t=eos,7c489799";
        let records = parse_import_csv(csv_data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            ImportRecord {
                email: "yoyo930021@gmail.com".to_string(),
                name: "yoyo930021".to_string(),
                ucode: "b3514a49".to_string(),
                status: true,
                verified_email: false,
                legacy_admin_link: "a8c11d7b".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_import_csv_v2() {
        let csv_data =
            "uid,mail,name,created_at\nb3514a49,yoyo930021@gmail.com,yoyo930021,1613500741";
        let records = parse_import_csv(csv_data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            ImportRecord {
                email: "yoyo930021@gmail.com".to_string(),
                name: "yoyo930021".to_string(),
                ucode: "b3514a49".to_string(),
                status: true,
                verified_email: true,
                legacy_admin_link: String::new(),
            }
        );
    }

    #[test]
    fn test_parse_import_csv_unknown_format() {
        let csv_data = "foo,bar,baz\n1,2,3";
        let result = parse_import_csv(csv_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_export_csv() {
        let records = vec![ExportCsvRecord {
            email: "test@example.com".to_string(),
            name: "Test".to_string(),
            ucode: "abc12345".to_string(),
            status: true,
            admin_link: "hashvalue".to_string(),
            openhash: "hmacvalue".to_string(),
        }];
        let output = write_export_csv(&records).unwrap();
        assert!(output.contains("test@example.com"));
        assert!(output.contains("Test"));
        assert!(output.contains("abc12345"));
    }
}
