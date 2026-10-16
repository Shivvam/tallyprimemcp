pub fn build_list_companies_xml() -> String {
    r#"<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Data</TYPE>
    <ID>List of Companies</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
      </STATICVARIABLES>
    </DESC>
  </BODY>
</ENVELOPE>"#
        .to_string()
}

pub fn build_list_ledgers_xml(company: &str) -> String {
    let company_escaped = quick_xml::escape::escape(company);
    format!(
        r#"<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Data</TYPE>
    <ID>List of Accounts</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        <ACCOUNTTYPE>Ledgers</ACCOUNTTYPE>
        <SVCURRENTCOMPANY>{}</SVCURRENTCOMPANY>
      </STATICVARIABLES>
    </DESC>
  </BODY>
</ENVELOPE>"#,
        company_escaped
    )
}

pub fn build_trial_balance_xml(
    company: &str,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> String {
    let company_escaped = quick_xml::escape::escape(company);
    let from_xml = from_date
        .map(|d| format!("<SVFROMDATE>{}</SVFROMDATE>", d))
        .unwrap_or_default();
    let to_xml = to_date
        .map(|d| format!("<SVTODATE>{}</SVTODATE>", d))
        .unwrap_or_default();
    format!(
        r#"<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Data</TYPE>
    <ID>Trial Balance</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        <SVCURRENTCOMPANY>{}</SVCURRENTCOMPANY>
        {}
        {}
      </STATICVARIABLES>
    </DESC>
  </BODY>
</ENVELOPE>"#,
        company_escaped, from_xml, to_xml
    )
}

pub fn build_day_book_xml(company: &str, from_date: Option<&str>, to_date: Option<&str>) -> String {
    let company_escaped = quick_xml::escape::escape(company);
    let from_xml = from_date
        .map(|d| format!("<SVFROMDATE>{}</SVFROMDATE>", d))
        .unwrap_or_default();
    let to_xml = to_date
        .map(|d| format!("<SVTODATE>{}</SVTODATE>", d))
        .unwrap_or_default();
    format!(
        r#"<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Data</TYPE>
    <ID>Day Book</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        <SVCURRENTCOMPANY>{}</SVCURRENTCOMPANY>
        {}
        {}
      </STATICVARIABLES>
    </DESC>
  </BODY>
</ENVELOPE>"#,
        company_escaped, from_xml, to_xml
    )
}
