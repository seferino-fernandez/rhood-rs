//! Account document model types.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Filterable document types accepted by the documents endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    /// Periodic account statement (monthly, quarterly).
    AccountStatement,
    /// Trade confirmation for a specific transaction.
    TradeConfirm,
    /// Consolidated 1099 tax form.
    ///
    /// The accepted filter value is the bare string `"1099"`, verified live:
    /// `?type=1099` returns HTTP 200, while the previously-guessed `"tax_1099"`
    /// returns HTTP 400.
    #[serde(rename = "1099")]
    #[cfg_attr(feature = "clap", value(name = "1099"))]
    Tax1099,
}

impl fmt::Display for DocumentType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountStatement => formatter.write_str("account_statement"),
            Self::TradeConfirm => formatter.write_str("trade_confirm"),
            Self::Tax1099 => formatter.write_str("1099"),
        }
    }
}

/// An account document (statement, tax form, trade confirmation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique document identifier.
    pub id: Option<String>,
    /// Document type (e.g., "account_statement", "trade_confirm").
    #[serde(rename = "type")]
    pub document_type: Option<String>,
    /// Date the document covers.
    pub date: Option<String>,
    /// Download URL.
    pub download_url: Option<String>,
    /// When the document was created.
    pub created_at: Option<String>,
    /// When the document was last updated.
    pub updated_at: Option<String>,
    /// API URL for the document.
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_type_serializes_to_wire_form() {
        assert_eq!(
            serde_json::to_string(&DocumentType::AccountStatement).unwrap(),
            "\"account_statement\""
        );
        assert_eq!(
            serde_json::to_string(&DocumentType::TradeConfirm).unwrap(),
            "\"trade_confirm\""
        );
        assert_eq!(
            serde_json::to_string(&DocumentType::Tax1099).unwrap(),
            "\"1099\""
        );
    }

    #[test]
    fn document_type_roundtrips_all_variants() {
        for variant in [
            DocumentType::AccountStatement,
            DocumentType::TradeConfirm,
            DocumentType::Tax1099,
        ] {
            let wire = serde_json::to_string(&variant).unwrap();
            let back: DocumentType = serde_json::from_str(&wire).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn document_type_display_matches_wire() {
        assert_eq!(
            DocumentType::AccountStatement.to_string(),
            "account_statement"
        );
        assert_eq!(DocumentType::TradeConfirm.to_string(), "trade_confirm");
        assert_eq!(DocumentType::Tax1099.to_string(), "1099");
    }
}
