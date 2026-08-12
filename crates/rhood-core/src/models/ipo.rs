//! IPO Access view models returned by Robinhood's Bonfire API.
//!
//! These responses are presentation models used by Robinhood's clients. Known
//! fields remain optional and nested values stay raw because their shape can
//! change with the currently available offerings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The IPO Access list view model for the authenticated user.
///
/// An account with no available offerings receives an `empty_state`. Accounts
/// with offerings can instead receive sections and other presentation data.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct IpoAccessList {
    /// Empty-state presentation shown when no offering is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_state: Option<serde_json::Value>,
    /// Educational content displayed alongside the offering list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learn_tab: Option<serde_json::Value>,
    /// Raw offering sections when IPOs are available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<serde_json::Value>,
    /// Additional top-level fields retained for forward compatibility.
    #[serde(default, flatten)]
    pub additional_fields: BTreeMap<String, serde_json::Value>,
}

/// A compact IPO Access offering card.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct IpoAccessCard {
    /// Robinhood instrument identifier for the offering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_id: Option<String>,
    /// Company or instrument name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Primary card title, commonly the ticker symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Secondary card text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Raw theme colors used to render the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<serde_json::Value>,
    /// Raw action triggered when the card is selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<serde_json::Value>,
    /// Additional card fields retained for forward compatibility.
    #[serde(default, flatten)]
    pub additional_fields: BTreeMap<String, serde_json::Value>,
}

/// Internal envelope returned by the IPO Access cards endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct IpoAccessCardsResponse {
    /// Cards returned for the requested instrument identifiers.
    #[serde(default)]
    pub results: Option<Vec<Option<IpoAccessCard>>>,
}

#[cfg(test)]
mod tests {
    use super::{IpoAccessCard, IpoAccessList};

    #[test]
    fn empty_list_capture_round_trips_without_losing_unknown_fields() {
        let expected = serde_json::json!({
            "empty_state": {
                "title": "No new IPOs available",
                "subtitle_markdown": "This list gets updated whenever a new IPO becomes available."
            },
            "learn_tab": {"sections": [{"section_type": "video", "section_data": {}}]},
            "future_component": {"retained": true}
        });

        let list: IpoAccessList = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(
            list.empty_state.as_ref().unwrap()["title"],
            "No new IPOs available"
        );
        assert_eq!(
            list.additional_fields["future_component"],
            expected["future_component"]
        );
        assert_eq!(serde_json::to_value(list).unwrap(), expected);
    }

    #[test]
    fn populated_list_sections_remain_raw() {
        let expected = serde_json::json!({
            "sections": [{"offering": {"instrument_id": "instrument-1"}}]
        });

        let list: IpoAccessList = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(
            list.sections.as_ref().unwrap()[0]["offering"]["instrument_id"],
            "instrument-1"
        );
        assert_eq!(serde_json::to_value(list).unwrap(), expected);
    }

    #[test]
    fn card_capture_round_trips_without_losing_unknown_fields() {
        let expected = serde_json::json!({
            "instrument_id": "450dfc6d-5510-4d40-abfb-f633b7d9be3e",
            "name": "Apple",
            "title": "AAPL",
            "subtitle": "",
            "accent_color": {"light": "fg", "dark": "fg"},
            "action": {"action_type": "deeplink", "action_data": {}},
            "future_badge": {"text": "New"}
        });

        let card: IpoAccessCard = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(card.title.as_deref(), Some("AAPL"));
        assert_eq!(
            card.additional_fields["future_badge"],
            expected["future_badge"]
        );
        assert_eq!(serde_json::to_value(card).unwrap(), expected);
    }
}
