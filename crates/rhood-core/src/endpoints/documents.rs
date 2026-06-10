use crate::Result;
use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::document::{Document, DocumentType};

impl RobinhoodClient {
    /// Fetches account documents, optionally filtered by type.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_documents(&self, doc_type: Option<DocumentType>) -> Result<Vec<Document>> {
        let url = self.api_url(paths::DOCUMENTS);
        match doc_type {
            Some(filter) => {
                let filter_string = filter.to_string();
                self.get_paginated(&url, &[("type", filter_string.as_str())])
                    .await
            }
            None => self.get_paginated(&url, &[]).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::models::document::Document;

    #[test]
    fn document_deserializes() {
        let json = r#"{
            "id": "doc-001",
            "type": "account_statement",
            "date": "2026-03-31",
            "download_url": "https://api.robinhood.com/documents/doc-001/download/",
            "created_at": "2026-04-01T00:00:00Z"
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.id.as_deref(), Some("doc-001"));
        assert_eq!(doc.document_type.as_deref(), Some("account_statement"));
        assert!(doc.download_url.is_some());
    }

    #[test]
    fn document_handles_missing_fields() {
        let json = r#"{"id": "doc-002"}"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.id.as_deref(), Some("doc-002"));
        assert!(doc.document_type.is_none());
        assert!(doc.date.is_none());
    }
}
