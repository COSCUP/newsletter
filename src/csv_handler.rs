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
