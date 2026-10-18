use crate::models::VoucherDraft;
use rust_decimal::Decimal;

pub fn build_voucher_import_xml(draft: &VoucherDraft) -> String {
    let company_escaped = quick_xml::escape::escape(&draft.company);
    let vch_type_escaped = quick_xml::escape::escape(&draft.voucher_type);
    let number_escaped = quick_xml::escape::escape(&draft.number);
    let party_escaped = quick_xml::escape::escape(&draft.party_ledger);
    let narration_escaped = quick_xml::escape::escape(&draft.narration);
    let guid = draft
        .tally_guid
        .as_deref()
        .unwrap_or(&draft.idempotency_key);

    let mut ledger_entries_xml = String::new();
    for entry in &draft.entries {
        let ledger_escaped = quick_xml::escape::escape(&entry.ledger);
        // In Tally:
        // Negative amount is typically Debit (ISDEEMEDPOSITIVE = Yes)
        // Positive amount is typically Credit (ISDEEMEDPOSITIVE = No)
        let is_deemed_positive = if entry.amount < Decimal::ZERO {
            "Yes"
        } else {
            "No"
        };
        let formatted_amount = entry.amount.abs().to_string();

        ledger_entries_xml.push_str(&format!(
            r#"        <ALLLEDGERENTRIES.LIST>
          <LEDGERNAME>{}</LEDGERNAME>
          <ISDEEMEDPOSITIVE>{}</ISDEEMEDPOSITIVE>
          <AMOUNT>{}</AMOUNT>
        </ALLLEDGERENTRIES.LIST>
"#,
            ledger_escaped, is_deemed_positive, formatted_amount
        ));
    }

    format!(
        r#"<ENVELOPE>
  <HEADER>
    <TALLYREQUEST>Import Data</TALLYREQUEST>
  </HEADER>
  <BODY>
    <IMPORTDATA>
      <REQUESTDESC>
        <REPORTNAME>Vouchers</REPORTNAME>
        <STATICVARIABLES>
          <SVCURRENTCOMPANY>{}</SVCURRENTCOMPANY>
        </STATICVARIABLES>
      </REQUESTDESC>
      <REQUESTDATA>
        <TALLYMESSAGE xmlns:UDF="TallyUDF">
          <VOUCHER VCHTYPE="{}" ACTION="Create">
            <GUID>{}</GUID>
            <DATE>{}</DATE>
            <EFFECTIVEDATE>{}</EFFECTIVEDATE>
            <VOUCHERTYPENAME>{}</VOUCHERTYPENAME>
            <VOUCHERNUMBER>{}</VOUCHERNUMBER>
            <PARTYLEDGERNAME>{}</PARTYLEDGERNAME>
            <NARRATION>{}</NARRATION>
{}          </VOUCHER>
        </TALLYMESSAGE>
      </REQUESTDATA>
    </IMPORTDATA>
  </BODY>
</ENVELOPE>"#,
        company_escaped,
        vch_type_escaped,
        guid,
        draft.date,
        draft.date,
        vch_type_escaped,
        number_escaped,
        party_escaped,
        narration_escaped,
        ledger_entries_xml
    )
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportResponse {
    pub created: u32,
    pub altered: u32,
    pub errors: u32,
    pub error_message: Option<String>,
}

pub fn parse_import_response(xml: &str) -> ImportResponse {
    let mut created = 0;
    let mut altered = 0;
    let mut errors = 0;
    let mut error_msg = None;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut current_tag = String::new();

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(ref e) => {
                current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Event::Text(e) => {
                if let Ok(text) = e.unescape() {
                    let trimmed = text.trim();
                    match current_tag.as_str() {
                        "CREATED" => {
                            created = trimmed.parse().unwrap_or(0);
                        }
                        "ALTERED" => {
                            altered = trimmed.parse().unwrap_or(0);
                        }
                        "ERRORS" => {
                            errors = trimmed.parse().unwrap_or(0);
                        }
                        "LINEERROR" | "EXCEPTIONS" if !trimmed.is_empty() => {
                            error_msg = Some(trimmed.to_string());
                        }
                        _ => {}
                    }
                }
            }
            Event::End(_) => {
                current_tag.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }

    ImportResponse {
        created,
        altered,
        errors,
        error_message: error_msg,
    }
}
