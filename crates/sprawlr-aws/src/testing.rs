//! Test doubles for the AWS SDK: scripted HTTP answers and a clock that moves only when the SDK
//! sleeps.

use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

type Script = dyn Fn(usize) -> (u16, &'static str) + Send + Sync;

/// Answers each call with what the script says for its number (from 0).
#[derive(Clone)]
pub(crate) struct Scripted(Arc<Script>, Arc<AtomicUsize>);

impl std::fmt::Debug for Scripted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scripted")
    }
}

impl HttpConnector for Scripted {
    fn call(&self, _: HttpRequest) -> HttpConnectorFuture {
        let (status, body) = (self.0)(self.1.fetch_add(1, Ordering::SeqCst));
        if status == 0 {
            let refused = aws_smithy_runtime_api::client::result::ConnectorError::io(body.into());
            return HttpConnectorFuture::ready(Err(refused));
        }
        let status = status.try_into().expect("a valid status");
        HttpConnectorFuture::ready(Ok(HttpResponse::new(status, aws_smithy_types::body::SdkBody::from(body))))
    }
}

impl HttpClient for Scripted {
    fn http_connector(&self, _: &HttpConnectorSettings, _: &RuntimeComponents) -> SharedHttpConnector {
        SharedHttpConnector::new(self.clone())
    }
}

/// Time that moves only when the SDK sleeps: backoff and adaptive pacing (which sleeps, then
/// checks the clock again) run as they would, without the wait.
#[derive(Debug, Clone)]
pub(crate) struct VirtualClock(Arc<std::sync::Mutex<std::time::SystemTime>>);

impl aws_smithy_async::rt::sleep::AsyncSleep for VirtualClock {
    fn sleep(&self, d: std::time::Duration) -> aws_smithy_async::rt::sleep::Sleep {
        *self.0.lock().unwrap() += d;
        aws_smithy_async::rt::sleep::Sleep::new(std::future::ready(()))
    }
}

impl aws_smithy_async::time::TimeSource for VirtualClock {
    fn now(&self) -> std::time::SystemTime {
        *self.0.lock().unwrap()
    }
}

/// Each test gets its own region: the SDK keeps retry and pacing state per service and
/// region for the whole process, so tests sharing one would depend on their order.
pub(crate) fn sdk_answering(region: &str, script: impl Fn(usize) -> (u16, &'static str) + Send + Sync + 'static) -> aws_config::SdkConfig {
    let creds = aws_credential_types::Credentials::new("id", "secret", None, None, "test");
    let clock = VirtualClock(Arc::new(std::sync::Mutex::new(std::time::SystemTime::now())));
    aws_config::SdkConfig::builder()
        .http_client(Scripted(Arc::new(script), Arc::default()))
        .sleep_impl(aws_smithy_async::rt::sleep::SharedAsyncSleep::new(clock.clone()))
        .time_source(aws_smithy_async::time::SharedTimeSource::new(clock))
        .region(aws_config::Region::new(region.to_string()))
        .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(creds))
        .behavior_version(aws_config::BehaviorVersion::latest())
        .build()
}
