pub mod account;
pub mod futures;
pub mod index;
pub mod login;
pub mod market;
pub mod option;
pub mod order;
pub mod recurring;
pub mod stock;
pub mod watchlist;

use rhood_core::RobinhoodClient;
use rhood_core::models::account::AccountDescriptor;

/// Ensure the client is logged in from the token cache, or bail with a user-facing message.
///
/// Uses `login_from_cache()` which already validates the cached token via a live
/// API call and attempts refresh on failure.
pub async fn ensure_logged_in(client: &RobinhoodClient) -> anyhow::Result<()> {
    if !client.login_from_cache().await? {
        anyhow::bail!("Not logged in. Run `rhood login` first.");
    }
    Ok(())
}

/// Resolves a caller-supplied account or the only discovered account.
pub async fn resolve_account_number(
    client: &RobinhoodClient,
    requested: Option<&str>,
) -> anyhow::Result<String> {
    let account = resolve_account(client, requested).await?;
    account
        .canonical_account_number()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("resolved account has no canonical account number"))
}

/// Resolves a caller-supplied account or the only discovered account.
pub async fn resolve_account(
    client: &RobinhoodClient,
    requested: Option<&str>,
) -> anyhow::Result<AccountDescriptor> {
    Ok(client.resolve_account(requested).await?)
}
