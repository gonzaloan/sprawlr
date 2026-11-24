//! All tags in the region in a few calls (instead of one ListTags call per resource).

use crate::{ScanContext, ScanOutput};
use sprawlr_core::NodeId;

pub async fn scan(ctx: &ScanContext) -> anyhow::Result<ScanOutput> {
    let client = aws_sdk_resourcegroupstagging::Client::new(&ctx.sdk);
    let mut pages = client.get_resources().resources_per_page(100).into_paginator().send();
    let mut out = ScanOutput { authoritative: true, ..Default::default() };
    while let Some(page) = pages.next().await {
        for m in page?.resource_tag_mapping_list() {
            let Some(arn) = m.resource_arn() else { continue };
            let tags = m.tags().iter().map(|t| (t.key().to_string(), t.value().to_string())).collect();
            out.tags.push((NodeId(arn.to_string()), tags));
        }
    }
    Ok(out)
}
