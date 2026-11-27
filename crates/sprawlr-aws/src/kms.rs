//! KMS key aliases. Cloud Control reads keys by id only; ListAliases (paginated, once per region)
//! names every key, AWS managed ones (`alias/aws/s3`) included.

use crate::cloudcontrol::failure;
use crate::ScanContext;

/// (alias, key id) for every alias that points at a key.
pub async fn aliases(ctx: &ScanContext) -> Result<Vec<(String, String)>, String> {
    let c = aws_sdk_kms::Client::new(&ctx.sdk);
    let mut pages = c.list_aliases().into_paginator().send();
    let mut out = Vec::new();
    while let Some(page) = pages.next().await {
        let page = page.map_err(|e| failure(&e).message)?;
        out.extend(page.aliases().iter().filter_map(|a| Some((a.alias_name()?.to_string(), a.target_key_id()?.to_string()))));
    }
    Ok(out)
}
