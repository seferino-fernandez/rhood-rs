use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::stock::{
    Fundamentals, HistoricalBounds, HistoricalInterval, HistoricalOpts, HistoricalSpan,
};
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum StockCommand {
    /// Get real-time quotes for one or more symbols
    Quote {
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Get historical price data (OHLCV candles)
    History {
        symbol: String,
        /// Candle interval
        #[arg(long, value_enum, default_value_t = HistoricalInterval::Hour)]
        interval: HistoricalInterval,
        /// Time span
        #[arg(long, value_enum, default_value_t = HistoricalSpan::Week)]
        span: HistoricalSpan,
        /// Trading hours
        #[arg(long, value_enum, default_value_t = HistoricalBounds::Regular)]
        bounds: HistoricalBounds,
    },
    /// Get fundamental data (sector, PE ratio, market cap, dividend yield)
    Fundamentals {
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Get latest trade price for symbols (uses ext-hours price when available)
    LatestPrices {
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Get earnings data for a symbol
    Earnings { symbol: String },
    /// Get analyst ratings for a symbol
    Ratings { symbol: String },
    /// Get recent news for a symbol
    News { symbol: String },
    /// Get stock split history for a symbol
    Splits { symbol: String },
    /// Get instruments associated with a tag (e.g., "100-most-popular")
    Tags { tag: String },
}

/// Format a raw market-cap string (e.g. `"4583496918000"`) into a human-readable
/// SI-suffixed value: `>=1e12 -> "{:.2}T"`, `>=1e9 -> "{:.2}B"`, `>=1e6 -> "{:.2}M"`,
/// else returns the raw string unchanged.  Falls back to the raw string on any
/// parse failure so no data is lost.
fn human_market_cap(raw: &str) -> String {
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 1e12 => format!("{:.2}T", v / 1e12),
        Ok(v) if v.is_finite() && v >= 1e9 => format!("{:.2}B", v / 1e9),
        Ok(v) if v.is_finite() && v >= 1e6 => format!("{:.2}M", v / 1e6),
        Ok(_) | Err(_) => raw.to_owned(),
    }
}

/// Format a raw dividend-yield string (e.g. `"0.33"`) as a percentage string
/// (`"0.33%"`).  The API already returns a percentage number so no multiplication
/// is applied.  Falls back to the raw string on any parse failure.
fn format_percent(raw: &str) -> String {
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => format!("{v:.2}%"),
        Ok(_) | Err(_) => raw.to_owned(),
    }
}

/// Format a raw price string (e.g. `"312.070000"`) to two decimal places
/// (`"312.07"`).  Falls back to the raw string on any parse failure so empty /
/// non-numeric values are preserved as-is.
fn format_money(raw: &str) -> String {
    match raw.trim().parse::<f64>() {
        Ok(v) if v.is_finite() => format!("{v:.2}"),
        Ok(_) | Err(_) => raw.to_owned(),
    }
}

/// Render a stock-split multiplier/divisor pair as a human-readable ratio
/// string (e.g. `"7:1"`), trimming trailing decimal zeros.
///
/// Falls back to `"{multiplier}/{divisor}"` when either value cannot be
/// parsed as a finite `f64` or the divisor is zero.
fn split_ratio(multiplier: &str, divisor: &str) -> String {
    let parse = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    match (parse(multiplier), parse(divisor)) {
        (Some(m), Some(d)) if d != 0.0 => {
            // Strip trailing zeros: format with enough precision then trim.
            let fmt_side = |v: f64| {
                let s = format!("{v:.8}");
                let s = s.trim_end_matches('0');
                let s = s.trim_end_matches('.');
                s.to_owned()
            };
            format!("{}:{}", fmt_side(m), fmt_side(d))
        }
        _ => format!("{multiplier}/{divisor}"),
    }
}

fn fundamentals_rows(fundamentals: &[Fundamentals]) -> Vec<Vec<String>> {
    fundamentals
        .iter()
        .map(|fundamental| {
            vec![
                fundamental.symbol.clone().unwrap_or_default(),
                fundamental.sector.clone().unwrap_or_default(),
                fundamental.pe_ratio.clone().unwrap_or_default(),
                human_market_cap(fundamental.market_cap.as_deref().unwrap_or_default()),
                format_percent(fundamental.dividend_yield.as_deref().unwrap_or_default()),
            ]
        })
        .collect()
}

fn tag_rows(
    result: &rhood_core::models::research::TagResult,
    requested_tag: &str,
) -> Vec<Vec<String>> {
    let tag_name = result.name.as_deref().unwrap_or(requested_tag);
    result
        .instruments
        .as_deref()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            let symbol = result
                .instrument_symbols
                .get(index)
                .and_then(Option::as_deref)
                .unwrap_or(reference);
            vec![tag_name.to_owned(), symbol.to_owned()]
        })
        .collect()
}

pub async fn run(
    cmd: &StockCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        StockCommand::Quote { symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            let quotes = client.get_quotes(&refs).await?;
            let headers = &["Symbol", "Last Price", "Bid", "Ask", "Prev Close"];
            let rows: Vec<Vec<String>> = quotes
                .iter()
                .map(|quote| {
                    vec![
                        quote.symbol.clone().unwrap_or_default(),
                        format_money(quote.last_trade_price.as_deref().unwrap_or_default()),
                        format_money(quote.bid_price.as_deref().unwrap_or_default()),
                        format_money(quote.ask_price.as_deref().unwrap_or_default()),
                        format_money(quote.previous_close.as_deref().unwrap_or_default()),
                    ]
                })
                .collect();
            output(format, headers, &rows, &quotes);
        }
        StockCommand::History {
            symbol,
            interval,
            span,
            bounds,
        } => {
            let opts = HistoricalOpts {
                interval: *interval,
                span: *span,
                bounds: *bounds,
            };
            let candles = client
                .get_stock_historicals(&[symbol.as_str()], &opts)
                .await?;
            let headers = &["Time", "Open", "Close", "High", "Low", "Volume"];
            let rows: Vec<Vec<String>> = candles
                .iter()
                .map(|candle| {
                    vec![
                        candle.begins_at.clone().unwrap_or_default(),
                        candle.open_price.clone().unwrap_or_default(),
                        candle.close_price.clone().unwrap_or_default(),
                        candle.high_price.clone().unwrap_or_default(),
                        candle.low_price.clone().unwrap_or_default(),
                        candle
                            .volume
                            .map(|volume| volume.to_string())
                            .unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &candles);
        }
        StockCommand::Fundamentals { symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            let funds = client.get_fundamentals(&refs).await?;
            let headers = &["Symbol", "Sector", "PE Ratio", "Market Cap", "Div Yield"];
            let rows = fundamentals_rows(&funds);
            output(format, headers, &rows, &funds);
        }
        StockCommand::LatestPrices { symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            let prices = client.get_latest_prices(&refs).await?;
            #[derive(serde::Serialize)]
            struct LatestPrice {
                symbol: String,
                price: String,
            }
            let records: Vec<LatestPrice> = prices
                .iter()
                .map(|(symbol, price)| LatestPrice {
                    symbol: symbol.clone(),
                    price: price.clone(),
                })
                .collect();
            let headers = &["Symbol", "Price"];
            let rows: Vec<Vec<String>> = records
                .iter()
                .map(|r| vec![r.symbol.clone(), r.price.clone()])
                .collect();
            output(format, headers, &rows, &records);
        }
        StockCommand::Earnings { symbol } => {
            let earnings = client.get_earnings(symbol).await?;
            let headers = &[
                "Symbol",
                "Year",
                "Qtr",
                "EPS Est",
                "EPS Actual",
                "Date",
                "Timing",
            ];
            let rows: Vec<Vec<String>> = earnings
                .iter()
                .map(|earning| {
                    vec![
                        earning.symbol.clone().unwrap_or_default(),
                        earning
                            .year
                            .map(|year| year.to_string())
                            .unwrap_or_default(),
                        earning
                            .quarter
                            .map(|quarter| quarter.to_string())
                            .unwrap_or_default(),
                        earning
                            .eps
                            .as_ref()
                            .and_then(|eps| eps.estimate.clone())
                            .unwrap_or_default(),
                        earning
                            .eps
                            .as_ref()
                            .and_then(|eps| eps.actual.clone())
                            .unwrap_or_default(),
                        earning
                            .report
                            .as_ref()
                            .and_then(|report| report.date.clone())
                            .unwrap_or_default(),
                        earning
                            .report
                            .as_ref()
                            .and_then(|report| report.timing.clone())
                            .unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &earnings);
        }
        StockCommand::Ratings { symbol } => {
            let rating = client.get_ratings(symbol).await?;
            if let Some(summary) = &rating.summary {
                let headers = &["Buy", "Hold", "Sell", "Total", "Buy %"];
                let rows = vec![vec![
                    summary
                        .num_buy_ratings
                        .map(|buy_rating| buy_rating.to_string())
                        .unwrap_or_default(),
                    summary
                        .num_hold_ratings
                        .map(|hold_rating| hold_rating.to_string())
                        .unwrap_or_default(),
                    summary
                        .num_sell_ratings
                        .map(|sell_rating| sell_rating.to_string())
                        .unwrap_or_default(),
                    summary.total().to_string(),
                    format!("{:.1}%", summary.buy_pct()),
                ]];
                output(format, headers, &rows, &rating);
            } else {
                output(
                    format,
                    &["Info"],
                    &[vec!["No ratings available".into()]],
                    &rating,
                );
            }
        }
        StockCommand::News { symbol } => {
            let articles = client.get_news(symbol).await?;
            let headers = &["Published", "Source", "Title"];
            let rows: Vec<Vec<String>> = articles
                .iter()
                .map(|news_article| {
                    vec![
                        news_article.published_at.clone().unwrap_or_default(),
                        news_article.source.clone().unwrap_or_default(),
                        news_article.title.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &articles);
        }
        StockCommand::Splits { symbol } => {
            let splits = client.get_splits(symbol).await?;
            let headers = &["Date", "Ratio"];
            let rows: Vec<Vec<String>> = splits
                .iter()
                .map(|stock_split| {
                    vec![
                        stock_split.execution_date.clone().unwrap_or_default(),
                        split_ratio(
                            stock_split.multiplier.as_deref().unwrap_or(""),
                            stock_split.divisor.as_deref().unwrap_or(""),
                        ),
                    ]
                })
                .collect();
            output(format, headers, &rows, &splits);
        }
        StockCommand::Tags { tag } => {
            let result = client.get_tags(tag).await?;
            let headers = &["Tag", "Symbol"];
            let rows = tag_rows(&result, tag);
            output(format, headers, &rows, &result);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        format_money, format_percent, fundamentals_rows, human_market_cap, split_ratio, tag_rows,
    };
    use rhood_core::models::{research::TagResult, stock::Fundamentals};

    // --- human_market_cap ---

    #[test]
    fn market_cap_trillions() {
        assert_eq!(human_market_cap("4583496918000"), "4.58T");
    }

    #[test]
    fn market_cap_billions() {
        assert_eq!(human_market_cap("2500000000"), "2.50B");
    }

    #[test]
    fn market_cap_millions() {
        // 750_000_000 = 7.5e8 (below 1e9) formats as millions
        assert_eq!(human_market_cap("750000000"), "750.00M");
        assert_eq!(human_market_cap("5000000"), "5.00M");
    }

    #[test]
    fn market_cap_below_million() {
        // Values below 1e6 are returned unchanged
        assert_eq!(human_market_cap("999999"), "999999");
    }

    #[test]
    fn market_cap_fallback_non_numeric() {
        assert_eq!(human_market_cap("n/a"), "n/a");
        assert_eq!(human_market_cap(""), "");
    }

    // --- format_percent ---

    #[test]
    fn percent_formats_correctly() {
        assert_eq!(format_percent("0.33"), "0.33%");
        assert_eq!(format_percent("1.5"), "1.50%");
        assert_eq!(format_percent("0"), "0.00%");
    }

    #[test]
    fn percent_fallback_non_numeric() {
        assert_eq!(format_percent("n/a"), "n/a");
        assert_eq!(format_percent(""), "");
    }

    // --- format_money ---

    #[test]
    fn money_trims_extra_decimals() {
        assert_eq!(format_money("312.070000"), "312.07");
        assert_eq!(format_money("100.000000"), "100.00");
    }

    #[test]
    fn money_empty_string_stays_raw() {
        assert_eq!(format_money(""), "");
    }

    #[test]
    fn money_fallback_non_numeric() {
        assert_eq!(format_money("N/A"), "N/A");
        assert_eq!(format_money("pending"), "pending");
    }

    #[test]
    fn split_ratio_happy_path() {
        // Standard 7-for-1 split: "7.00000000" / "1.00000000" → "7:1"
        assert_eq!(split_ratio("7.00000000", "1.00000000"), "7:1");
    }

    #[test]
    fn split_ratio_two_for_one() {
        assert_eq!(split_ratio("2.00000000", "1.00000000"), "2:1");
    }

    #[test]
    fn split_ratio_three_for_two() {
        // 3:2 split, divisor is not 1
        assert_eq!(split_ratio("3.00000000", "2.00000000"), "3:2");
    }

    #[test]
    fn split_ratio_non_integer_multiplier() {
        // 1.5 : 1, trailing zeros trimmed but decimal preserved
        assert_eq!(split_ratio("1.50000000", "1.00000000"), "1.5:1");
    }

    #[test]
    fn split_ratio_fallback_non_numeric() {
        // Non-numeric input falls back to raw "m/d" form
        assert_eq!(split_ratio("seven", "one"), "seven/one");
    }

    #[test]
    fn split_ratio_fallback_zero_divisor() {
        // Zero divisor falls back to raw form
        assert_eq!(
            split_ratio("7.00000000", "0.00000000"),
            "7.00000000/0.00000000"
        );
    }

    #[test]
    fn split_ratio_fallback_empty() {
        // Empty strings (missing fields) fall back to raw "/" form
        assert_eq!(split_ratio("", ""), "/");
    }

    #[test]
    fn fundamentals_omission_does_not_relabel_later_record() {
        let core_record = Fundamentals {
            symbol: Some("MSFT".to_owned()),
            sector: Some("Technology".to_owned()),
            ..Default::default()
        };

        // Core omitted the earlier requested AAPL record. CLI presentation must
        // retain the authoritative symbol on the returned later record.
        let rows = fundamentals_rows(&[core_record]);

        assert_eq!(rows, vec![vec!["MSFT", "Technology", "", "", ""]]);
    }

    #[test]
    fn tag_rows_preserve_positional_misses() {
        let result = TagResult {
            name: Some("Technology".to_owned()),
            slug: Some("technology".to_owned()),
            instruments: Some(vec![
                "https://api.robinhood.com/instruments/aapl/".to_owned(),
                "https://api.robinhood.com/instruments/missing/".to_owned(),
                "https://api.robinhood.com/instruments/msft/".to_owned(),
            ]),
            instrument_symbols: vec![Some("AAPL".to_owned()), None, Some("MSFT".to_owned())],
        };

        assert_eq!(
            tag_rows(&result, "requested-tag"),
            vec![
                vec!["Technology", "AAPL"],
                vec![
                    "Technology",
                    "https://api.robinhood.com/instruments/missing/"
                ],
                vec!["Technology", "MSFT"],
            ]
        );
    }

    #[test]
    fn tag_json_preserves_core_references_and_aligned_symbols() {
        let result = TagResult {
            name: Some("Technology".to_owned()),
            slug: Some("technology".to_owned()),
            instruments: Some(vec![
                "https://api.robinhood.com/instruments/aapl/".to_owned(),
                "https://api.robinhood.com/instruments/missing/".to_owned(),
            ]),
            instrument_symbols: vec![Some("AAPL".to_owned()), None],
        };

        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::json!({
                "name": "Technology",
                "slug": "technology",
                "instruments": [
                    "https://api.robinhood.com/instruments/aapl/",
                    "https://api.robinhood.com/instruments/missing/"
                ],
                "instrument_symbols": ["AAPL", null]
            })
        );
    }
}
