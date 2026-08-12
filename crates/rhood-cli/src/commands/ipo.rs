use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::ipo::{IpoAccessCard, IpoAccessList};
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum IpoCommand {
    /// Show the IPO Access offering list or its current empty state
    List,
    /// Show IPO Access cards for Robinhood instrument IDs
    Cards {
        /// Robinhood instrument UUIDs
        #[arg(required = true, value_parser = crate::utils::validation::parse_uuid)]
        instrument_ids: Vec<String>,
    },
}

pub async fn run(
    cmd: &IpoCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        IpoCommand::List => {
            let list = client.get_ipo_access_list().await?;
            let headers = &["IPO Access List"];
            let rows = ipo_access_list_rows(&list)?;
            output(format, headers, &rows, &list);
        }
        IpoCommand::Cards { instrument_ids } => {
            let id_refs: Vec<&str> = instrument_ids.iter().map(String::as_str).collect();
            let cards = client.get_ipo_access_cards(&id_refs).await?;
            let headers = &["Instrument ID", "Title", "Name", "Subtitle"];
            let rows = ipo_access_card_rows(&cards);
            output(format, headers, &rows, &cards);
        }
    }
    Ok(())
}

fn ipo_access_list_rows(list: &IpoAccessList) -> serde_json::Result<Vec<Vec<String>>> {
    serde_json::to_string(list).map(|json| vec![vec![json]])
}

fn ipo_access_card_rows(cards: &[IpoAccessCard]) -> Vec<Vec<String>> {
    cards
        .iter()
        .map(|card| {
            vec![
                card.instrument_id.clone().unwrap_or_default(),
                card.title.clone().unwrap_or_default(),
                card.name.clone().unwrap_or_default(),
                card.subtitle.clone().unwrap_or_default(),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{IpoCommand, ipo_access_card_rows, ipo_access_list_rows};
    use clap::{Parser, Subcommand};
    use rhood_core::models::ipo::{IpoAccessCard, IpoAccessList};

    const INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Subcommand)]
    enum TestCommand {
        Ipo {
            #[command(subcommand)]
            command: IpoCommand,
        },
    }

    #[test]
    fn list_and_cards_subcommands_validate_their_inputs() {
        assert!(matches!(
            TestCli::try_parse_from(["rhood", "ipo", "list"])
                .unwrap()
                .command,
            TestCommand::Ipo {
                command: IpoCommand::List
            }
        ));
        let cards = TestCli::try_parse_from(["rhood", "ipo", "cards", INSTRUMENT_ID]).unwrap();
        assert!(matches!(
            cards.command,
            TestCommand::Ipo {
                command: IpoCommand::Cards { instrument_ids }
            } if instrument_ids == [INSTRUMENT_ID]
        ));
        assert!(TestCli::try_parse_from(["rhood", "ipo", "cards"]).is_err());
        assert!(TestCli::try_parse_from(["rhood", "ipo", "cards", "not-a-uuid"]).is_err());
    }

    #[test]
    fn list_row_preserves_the_complete_raw_view_model() {
        let raw = serde_json::json!({
            "empty_state": {"title": "No new IPOs available"},
            "future_component": {"retained": true}
        });
        let list: IpoAccessList = serde_json::from_value(raw).unwrap();

        let rows = ipo_access_list_rows(&list).unwrap();
        let rendered: serde_json::Value = serde_json::from_str(&rows[0][0]).unwrap();

        assert_eq!(rendered["empty_state"]["title"], "No new IPOs available");
        assert_eq!(rendered["future_component"]["retained"], true);
    }

    #[test]
    fn card_rows_project_known_fields_and_json_retains_unknown_fields() {
        let raw = serde_json::json!({
            "instrument_id": INSTRUMENT_ID,
            "title": "AAPL",
            "name": "Apple",
            "future_badge": {"text": "New"}
        });
        let card: IpoAccessCard = serde_json::from_value(raw.clone()).unwrap();

        assert_eq!(
            ipo_access_card_rows(std::slice::from_ref(&card)),
            vec![vec![
                INSTRUMENT_ID.to_string(),
                "AAPL".to_string(),
                "Apple".to_string(),
                String::new()
            ]]
        );
        assert_eq!(
            serde_json::to_value(card).unwrap()["future_badge"],
            raw["future_badge"]
        );
    }
}
