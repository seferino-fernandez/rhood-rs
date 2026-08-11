use std::collections::HashSet;

use crate::{Result, RhoodError};

#[derive(Clone, Debug)]
pub(super) struct PlannedInput {
    pub(super) value: String,
    pub(super) ordinal: usize,
}

pub(super) enum ChunkLimit<'a> {
    ItemCount(usize),
    EncodedUrlBytes {
        maximum: usize,
        url: &'a str,
        parameter: &'a str,
        fixed_parameters: &'a [(&'a str, &'a str)],
    },
}

/// Stable-deduplicates inputs and plans sequential endpoint request chunks.
pub(super) fn plan_request_chunks(
    endpoint: &str,
    inputs: &[String],
    limit: ChunkLimit<'_>,
) -> Result<Vec<Vec<PlannedInput>>> {
    let mut seen = HashSet::with_capacity(inputs.len());
    let mut unique = Vec::with_capacity(inputs.len());
    for (index, value) in inputs.iter().enumerate() {
        if seen.insert(value.clone()) {
            unique.push(PlannedInput {
                value: value.clone(),
                ordinal: index.saturating_add(1),
            });
        }
    }

    match limit {
        ChunkLimit::ItemCount(maximum) => {
            debug_assert!(maximum > 0);
            Ok(unique
                .chunks(maximum)
                .map(<[PlannedInput]>::to_vec)
                .collect())
        }
        ChunkLimit::EncodedUrlBytes {
            maximum,
            url,
            parameter,
            fixed_parameters,
        } => plan_url_chunks(endpoint, unique, maximum, url, parameter, fixed_parameters),
    }
}

fn plan_url_chunks(
    endpoint: &str,
    inputs: Vec<PlannedInput>,
    maximum: usize,
    url: &str,
    parameter: &str,
    fixed_parameters: &[(&str, &str)],
) -> Result<Vec<Vec<PlannedInput>>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();

    for input in inputs {
        current.push(input);
        if encoded_url_length(url, parameter, &current, fixed_parameters)? <= maximum {
            continue;
        }

        let Some(overflowing) = current.pop() else {
            return Err(RhoodError::InvalidParameter(format!(
                "{endpoint} request chunk planning failed"
            )));
        };
        if current.is_empty() {
            return Err(oversized_input(endpoint, overflowing.ordinal));
        }
        chunks.push(std::mem::take(&mut current));
        current.push(overflowing);

        if encoded_url_length(url, parameter, &current, fixed_parameters)? > maximum {
            let input = current.first().ok_or_else(|| {
                RhoodError::InvalidParameter(format!("{endpoint} request chunk planning failed"))
            })?;
            return Err(oversized_input(endpoint, input.ordinal));
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

fn encoded_url_length(
    base: &str,
    parameter: &str,
    inputs: &[PlannedInput],
    fixed_parameters: &[(&str, &str)],
) -> Result<usize> {
    let mut url = reqwest::Url::parse(base).map_err(|_parse_error| {
        RhoodError::InvalidParameter("endpoint request URL is not a valid absolute URL".into())
    })?;
    let joined = inputs
        .iter()
        .map(|input| input.value.as_str())
        .collect::<Vec<_>>()
        .join(",");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair(parameter, &joined);
        query.extend_pairs(fixed_parameters.iter().copied());
    }
    Ok(url.as_str().len())
}

fn oversized_input(endpoint: &str, ordinal: usize) -> RhoodError {
    RhoodError::InvalidParameter(format!(
        "{endpoint} input at ordinal {ordinal} exceeds the encoded URL limit"
    ))
}

pub(super) fn chunk_failed(
    endpoint: &str,
    chunk_index: usize,
    total_chunks: usize,
    source: RhoodError,
) -> RhoodError {
    match source {
        RhoodError::NotAuthenticated => RhoodError::NotAuthenticated,
        source => RhoodError::RequestChunkFailed {
            endpoint: endpoint.to_string(),
            chunk_index,
            total_chunks,
            source: Box::new(source),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_batch_item_limit_is_stable_and_deduplicated() {
        let inputs = ["A", "B", "A", "C"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let chunks = plan_request_chunks("test", &inputs, ChunkLimit::ItemCount(2)).unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0][0].value, "A");
        assert_eq!(chunks[0][0].ordinal, 1);
        assert_eq!(chunks[0][1].value, "B");
        assert_eq!(chunks[1][0].value, "C");
        assert_eq!(chunks[1][0].ordinal, 4);
    }

    #[test]
    fn request_batch_url_limit_counts_encoding_and_fixed_parameters() {
        let inputs = vec!["A B".to_string(), "C/D".to_string()];
        let base = "https://example.test/marketdata/historicals/";
        let fixed = [
            ("interval", "5minute"),
            ("span", "day"),
            ("bounds", "regular"),
        ];
        let one = vec![PlannedInput {
            value: inputs[0].clone(),
            ordinal: 1,
        }];
        let two = vec![PlannedInput {
            value: inputs[1].clone(),
            ordinal: 2,
        }];
        let maximum = encoded_url_length(base, "symbols", &one, &fixed)
            .unwrap()
            .max(encoded_url_length(base, "symbols", &two, &fixed).unwrap());

        let chunks = plan_request_chunks(
            "stock historicals",
            &inputs,
            ChunkLimit::EncodedUrlBytes {
                maximum,
                url: base,
                parameter: "symbols",
                fixed_parameters: &fixed,
            },
        )
        .unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0][0].value, "A B");
        assert_eq!(chunks[1][0].value, "C/D");
    }

    #[test]
    fn request_batch_rejects_oversized_single_input_without_echoing_it() {
        let secret = "private value with spaces".to_string();
        let error = plan_request_chunks(
            "quotes",
            std::slice::from_ref(&secret),
            ChunkLimit::EncodedUrlBytes {
                maximum: 20,
                url: "https://example.test/quotes/",
                parameter: "symbols",
                fixed_parameters: &[],
            },
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("quotes"));
        assert!(message.contains("ordinal 1"));
        assert!(!message.contains(&secret));
    }

    #[test]
    fn request_batch_empty_input_has_no_chunks() {
        let chunks = plan_request_chunks("test", &[], ChunkLimit::ItemCount(17)).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn request_batch_does_not_wrap_pre_dispatch_authentication_failure() {
        let error = chunk_failed("quotes", 1, 2, RhoodError::NotAuthenticated);

        assert!(matches!(error, RhoodError::NotAuthenticated));
    }
}
