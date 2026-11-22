//! Adapter construction.
//!
//! The factory owns one HTTP client (connection pool, TLS configuration) and
//! derives an adapter per provider, so rebuilding the registry after a
//! configuration hot reload does not create new sockets pools.
//!
//! It also supports **explicit bindings** (`bind`): a host or a test can pin a
//! specific [`ProviderAdapter`] to a provider id — this is how `MockAdapter` is
//! injected, and how a custom/private protocol is plugged in without touching the
//! factory's protocol match. Bindings survive registry rebuilds, so a hot reload
//! keeps the mock in place.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use reqwest::Client;

use crate::error::{CoreError, CoreResult};
use crate::provider::adapter::ProviderAdapter;
use crate::provider::adapters::{AnthropicAdapter, OpenAiCompatibleAdapter};
use crate::provider::config::{ProviderConfig, ProviderId, ProviderProtocol};
use crate::provider::http;

/// Creates and caches provider adapters.
#[derive(Clone)]
pub struct AdapterFactory {
    client: Client,
    bindings: Arc<RwLock<BTreeMap<ProviderId, Arc<dyn ProviderAdapter>>>>,
}

impl AdapterFactory {
    /// Creates a factory with a fresh HTTP client.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Internal`] if the client cannot be built.
    pub fn new() -> CoreResult<Self> {
        Ok(Self {
            client: http::build_client()?,
            bindings: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    /// Pins `adapter` to `provider`, overriding the protocol default.
    ///
    /// The binding wins over the protocol match in [`AdapterFactory::adapter_for`]
    /// and is preserved across registry rebuilds.
    pub fn bind(&self, provider: ProviderId, adapter: Arc<dyn ProviderAdapter>) {
        match self.bindings.write() {
            Ok(mut bindings) => {
                bindings.insert(provider, adapter);
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(provider, adapter);
            }
        }
    }

    /// Removes an explicit binding.
    pub fn unbind(&self, provider: &ProviderId) {
        match self.bindings.write() {
            Ok(mut bindings) => {
                bindings.remove(provider);
            }
            Err(poisoned) => {
                poisoned.into_inner().remove(provider);
            }
        }
    }

    /// Returns the adapter bound to `provider`, if any.
    #[must_use]
    pub fn bound(&self, provider: &ProviderId) -> Option<Arc<dyn ProviderAdapter>> {
        match self.bindings.read() {
            Ok(bindings) => bindings.get(provider).cloned(),
            Err(poisoned) => poisoned.into_inner().get(provider).cloned(),
        }
    }

    /// Provider ids with an explicit binding.
    #[must_use]
    pub fn bound_providers(&self) -> Vec<ProviderId> {
        match self.bindings.read() {
            Ok(bindings) => bindings.keys().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().keys().cloned().collect(),
        }
    }

    /// Resolves the adapter responsible for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the configured protocol has no adapter
    /// and no explicit binding exists.
    pub fn adapter_for(&self, config: &ProviderConfig) -> CoreResult<Arc<dyn ProviderAdapter>> {
        if let Some(adapter) = self.bound(&config.id) {
            return Ok(adapter);
        }
        // `ProviderProtocol` is `#[non_exhaustive]`: within this crate the extra
        // arm is unreachable today, but it keeps a future variant from silently
        // falling back to a wrong adapter.
        #[allow(unreachable_patterns)]
        let adapter: Arc<dyn ProviderAdapter> = match config.protocol {
            ProviderProtocol::OpenAiCompatible => {
                Arc::new(OpenAiCompatibleAdapter::from_client(self.client.clone()))
            }
            ProviderProtocol::Anthropic => {
                Arc::new(AnthropicAdapter::from_client(self.client.clone()))
            }
            other => {
                return Err(CoreError::config_field(
                    format!("providers.{}.protocol", config.id),
                    format!("no adapter is registered for protocol `{other}`"),
                ));
            }
        };
        Ok(adapter)
    }
}

impl fmt::Debug for AdapterFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdapterFactory")
            .field("bound_providers", &self.bound_providers())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::adapters::MockAdapter;
    use crate::provider::config::ModelConfig;

    fn provider_config() -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new("demo").unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_model(ModelConfig::new("demo-model"))
    }

    #[test]
    fn protocol_defaults_are_used_without_bindings() {
        let factory = AdapterFactory::new().unwrap();
        let adapter = factory.adapter_for(&provider_config()).unwrap();
        assert_eq!(adapter.protocol(), ProviderProtocol::OpenAiCompatible);
        assert_eq!(adapter.label(), "openai-compatible");
    }

    #[test]
    fn explicit_bindings_win() {
        let factory = AdapterFactory::new().unwrap();
        let provider = ProviderId::new("demo").unwrap();
        factory.bind(
            provider.clone(),
            Arc::new(MockAdapter::new(provider.clone())),
        );
        let adapter = factory.adapter_for(&provider_config()).unwrap();
        assert_eq!(adapter.label(), "mock");
        assert_eq!(factory.bound_providers(), vec![provider.clone()]);

        factory.unbind(&provider);
        assert!(factory.bound(&provider).is_none());
    }
}
