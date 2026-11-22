//! Provider and model registries.
//!
//! A [`ProviderRegistry`] is an **immutable view** of one configuration snapshot:
//! the providers that can be routed to, the adapter handling each of them, and the
//! indexed models with their capabilities. Building a registry is the only place
//! that turns configuration into runnable adapters.
//!
//! # Hot reload semantics
//!
//! Nothing is mutated in place. A configuration change publishes a new
//! [`crate::ConfigSnapshot`]; [`crate::provider::ProviderRuntime`] then builds a new
//! registry for the new version and swaps the `Arc`. Requests that already hold
//! the previous `Arc` keep using it to the end, which is exactly the
//! "in-flight request keeps its configuration" requirement.
//!
//! # Enable/disable
//!
//! [`ProviderRegistry::set_provider_enabled`] and
//! [`ProviderRegistry::set_model_enabled`] mutate the registry in place (it is
//! owned, not shared, until it is published). They affect *routing only*; the
//! configuration snapshot stays untouched.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::config::{ConfigSnapshot, ConfigVersion};
use crate::error::{CoreError, CoreResult};
use crate::provider::adapter::ProviderAdapter;
use crate::provider::capabilities::{check_capabilities, RequestRequirements};
use crate::provider::config::{
    ModelCapabilities, ModelConfig, ModelSpec, ProviderConfig, ProviderId,
};
use crate::provider::factory::AdapterFactory;

/// A resolved model: the provider it belongs to plus its configuration.
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    provider_id: ProviderId,
    model: ModelConfig,
}

impl ModelDescriptor {
    pub(crate) fn new(provider_id: ProviderId, model: ModelConfig) -> Self {
        Self { provider_id, model }
    }

    /// Provider exposing the model.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Model configuration (capabilities, fallbacks, enabled flag).
    #[must_use]
    pub fn model(&self) -> &ModelConfig {
        &self.model
    }

    /// Wire model id.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model.id
    }

    /// Declared capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &ModelCapabilities {
        &self.model.capabilities
    }

    /// Whether the model may currently be routed to.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.model.enabled
    }

    /// `provider/model` spec identifying this model.
    #[must_use]
    pub fn spec(&self) -> ModelSpec {
        ModelSpec::new_unchecked(self.provider_id.clone(), self.model.id.clone())
    }
}

/// Index of every routable model, keyed by `provider/model`.
///
/// Kept separate from [`ProviderRegistry`] because transport questions ("who can
/// serve this?") and capability questions ("can it do tools?") have different
/// consumers and lifetimes.
#[derive(Debug, Default, Clone)]
pub struct ModelRegistry {
    models: BTreeMap<ModelSpec, Arc<ModelDescriptor>>,
    defaults: BTreeMap<ProviderId, ModelSpec>,
}

impl ModelRegistry {
    /// Looks up a model.
    #[must_use]
    pub fn get(&self, spec: &ModelSpec) -> Option<&Arc<ModelDescriptor>> {
        self.models.get(spec)
    }

    /// Whether the model exists (enabled or not).
    #[must_use]
    pub fn contains(&self, spec: &ModelSpec) -> bool {
        self.models.contains_key(spec)
    }

    /// Number of indexed models.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether no model is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// All indexed models.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<ModelDescriptor>> {
        self.models.values()
    }

    /// Models of one provider.
    pub fn for_provider<'a>(
        &'a self,
        provider: &'a ProviderId,
    ) -> impl Iterator<Item = &'a Arc<ModelDescriptor>> + 'a {
        self.models
            .values()
            .filter(move |descriptor| descriptor.provider_id() == provider)
    }

    /// Default model spec declared by a provider.
    #[must_use]
    pub fn default_spec(&self, provider: &ProviderId) -> Option<&ModelSpec> {
        self.defaults.get(provider)
    }

    /// Default model descriptor of a provider.
    #[must_use]
    pub fn default_descriptor(&self, provider: &ProviderId) -> Option<&Arc<ModelDescriptor>> {
        self.defaults
            .get(provider)
            .and_then(|spec| self.models.get(spec))
    }

    pub(crate) fn insert(&mut self, descriptor: ModelDescriptor) -> Arc<ModelDescriptor> {
        let spec = descriptor.spec();
        let descriptor = Arc::new(descriptor);
        self.models.insert(spec, Arc::clone(&descriptor));
        descriptor
    }

    pub(crate) fn set_default(&mut self, provider: ProviderId, spec: ModelSpec) {
        self.defaults.insert(provider, spec);
    }

    pub(crate) fn descriptor_mut(&mut self, spec: &ModelSpec) -> Option<&mut ModelDescriptor> {
        self.models.get_mut(spec).map(Arc::make_mut)
    }
}

/// One routable provider: its configuration and the adapter that talks to it.
#[derive(Clone)]
pub struct ProviderRecord {
    config: Arc<ProviderConfig>,
    adapter: Arc<dyn ProviderAdapter>,
    enabled: bool,
}

impl ProviderRecord {
    /// Provider id.
    #[must_use]
    pub fn id(&self) -> &ProviderId {
        &self.config.id
    }

    /// Provider configuration.
    #[must_use]
    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// Shared configuration handle (cheap to clone for a spawned task).
    #[must_use]
    pub fn shared_config(&self) -> Arc<ProviderConfig> {
        Arc::clone(&self.config)
    }

    /// Adapter serving this provider.
    #[must_use]
    pub fn adapter(&self) -> &Arc<dyn ProviderAdapter> {
        &self.adapter
    }

    /// Whether the provider may currently be routed to.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl fmt::Debug for ProviderRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRecord")
            .field("id", &self.config.id)
            .field("protocol", &self.config.protocol)
            .field("adapter", &self.adapter.label())
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Immutable view of the providers and models of one configuration snapshot.
#[derive(Debug)]
pub struct ProviderRegistry {
    version: ConfigVersion,
    providers: BTreeMap<ProviderId, ProviderRecord>,
    models: ModelRegistry,
}

impl ProviderRegistry {
    /// Creates an empty registry carrying `version`.
    #[must_use]
    pub fn empty(version: ConfigVersion) -> Self {
        Self {
            version,
            providers: BTreeMap::new(),
            models: ModelRegistry::default(),
        }
    }

    /// Builds the registry described by a configuration snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the snapshot is invalid or a provider's
    /// protocol has no registered adapter.
    pub fn from_snapshot(snapshot: &ConfigSnapshot, factory: &AdapterFactory) -> CoreResult<Self> {
        let mut registry = Self::empty(snapshot.version());
        for config in snapshot.config().providers.values() {
            let adapter = factory.adapter_for(config)?;
            registry.register(config.clone(), adapter)?;
        }
        Ok(registry)
    }

    /// Version of the configuration this registry was built from.
    #[must_use]
    pub fn version(&self) -> ConfigVersion {
        self.version
    }

    /// Number of registered providers (enabled or not).
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether no provider is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Registers (or replaces) a provider and indexes its models.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when the configuration is invalid, when the
    /// adapter speaks a different protocol than configured, or when a model of the
    /// provider is already registered under a different adapter.
    pub fn register(
        &mut self,
        config: ProviderConfig,
        adapter: Arc<dyn ProviderAdapter>,
    ) -> CoreResult<()> {
        config.validate()?;
        if adapter.protocol() != config.protocol {
            return Err(CoreError::config_field(
                format!("providers.{}.protocol", config.id),
                format!(
                    "adapter `{}` speaks `{}` but the provider is configured as `{}`",
                    adapter.label(),
                    adapter.protocol(),
                    config.protocol
                ),
            ));
        }

        let id = config.id.clone();
        let mut descriptors = Vec::with_capacity(config.models.len());
        for model in &config.models {
            let spec = ModelSpec::new_unchecked(id.clone(), model.id.clone());
            if self.models.contains(&spec) {
                return Err(CoreError::config_field(
                    format!("providers.{id}.models"),
                    format!("model `{}` is already registered", model.id),
                ));
            }
            descriptors.push(ModelDescriptor::new(id.clone(), model.clone()));
        }

        if let Some(default) = config.default_model_config() {
            let spec = ModelSpec::new_unchecked(id.clone(), default.id.clone());
            self.models.set_default(id.clone(), spec);
        }
        for descriptor in descriptors {
            self.models.insert(descriptor);
        }

        self.providers.insert(
            id,
            ProviderRecord {
                enabled: config.enabled,
                config: Arc::new(config),
                adapter,
            },
        );
        Ok(())
    }

    /// Enables or disables a provider for routing.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NotFound`] when the provider is unknown.
    pub fn set_provider_enabled(&mut self, provider: &ProviderId, enabled: bool) -> CoreResult<()> {
        let record = self
            .providers
            .get_mut(provider)
            .ok_or_else(|| CoreError::not_found(format!("provider `{provider}`")))?;
        record.enabled = enabled;
        Ok(())
    }

    /// Enables or disables a model for routing.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NotFound`] when the model is unknown.
    pub fn set_model_enabled(&mut self, spec: &ModelSpec, enabled: bool) -> CoreResult<()> {
        let descriptor = self
            .models
            .descriptor_mut(spec)
            .ok_or_else(|| CoreError::not_found(format!("model `{spec}`")))?;
        descriptor.model.enabled = enabled;
        Ok(())
    }

    /// Looks up a provider record.
    #[must_use]
    pub fn provider(&self, provider: &ProviderId) -> Option<&ProviderRecord> {
        self.providers.get(provider)
    }

    /// All providers, in alphabetical order.
    pub fn providers(&self) -> impl Iterator<Item = &ProviderRecord> {
        self.providers.values()
    }

    /// Providers currently enabled for routing.
    pub fn enabled_providers(&self) -> impl Iterator<Item = &ProviderRecord> {
        self.providers.values().filter(|record| record.enabled)
    }

    /// Model index.
    #[must_use]
    pub fn models(&self) -> &ModelRegistry {
        &self.models
    }

    /// Resolves the model a request should target.
    ///
    /// * explicit `provider/model` → that model, if the provider and model are
    ///   enabled;
    /// * otherwise → the default model of the first enabled provider.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] when nothing is configured or the requested
    /// entity is unknown/disabled.
    pub fn resolve(&self, requested: Option<&ModelSpec>) -> CoreResult<Arc<ModelDescriptor>> {
        match requested {
            Some(spec) => {
                let record = self.providers.get(spec.provider()).ok_or_else(|| {
                    CoreError::config_field(
                        "providers",
                        format!("unknown provider `{}`", spec.provider()),
                    )
                })?;
                if !record.enabled {
                    return Err(CoreError::config_field(
                        format!("providers.{}.enabled", spec.provider()),
                        "provider is disabled",
                    ));
                }
                let descriptor = self.models.get(spec).ok_or_else(|| {
                    CoreError::config_field(
                        format!("providers.{}.models", spec.provider()),
                        format!("unknown model `{}`", spec.model()),
                    )
                })?;
                if !descriptor.is_enabled() {
                    return Err(CoreError::config_field(
                        format!("providers.{}.models.{}", spec.provider(), spec.model()),
                        "model is disabled",
                    ));
                }
                Ok(Arc::clone(descriptor))
            }
            None => {
                for record in self.enabled_providers() {
                    if let Some(descriptor) = self.models.default_descriptor(record.id()) {
                        if descriptor.is_enabled() {
                            return Ok(Arc::clone(descriptor));
                        }
                    }
                }
                Err(CoreError::config_field(
                    "providers",
                    "no enabled provider with an enabled default model",
                ))
            }
        }
    }

    /// Validates a resolved model against request requirements.
    ///
    /// # Errors
    ///
    /// * [`CoreError::Config`] when the provider/model is disabled;
    /// * [`CoreError::CapabilityUnsupported`] / [`CoreError::TokenLimitExceeded`]
    ///   from [`check_capabilities`].
    pub fn check_requirements(
        &self,
        descriptor: &ModelDescriptor,
        requirements: &RequestRequirements,
    ) -> CoreResult<()> {
        let record = self
            .providers
            .get(descriptor.provider_id())
            .ok_or_else(|| {
                CoreError::config_field(
                    "providers",
                    format!("unknown provider `{}`", descriptor.provider_id()),
                )
            })?;
        if !record.enabled {
            return Err(CoreError::config_field(
                format!("providers.{}.enabled", descriptor.provider_id()),
                "provider is disabled",
            ));
        }
        if !descriptor.is_enabled() {
            return Err(CoreError::config_field(
                format!(
                    "providers.{}.models.{}",
                    descriptor.provider_id(),
                    descriptor.model_id()
                ),
                "model is disabled",
            ));
        }
        check_capabilities(
            descriptor.provider_id(),
            descriptor.model_id(),
            descriptor.capabilities(),
            requirements,
        )
    }

    /// Fallback models declared by `primary`, in order, skipping anything that is
    /// disabled, duplicated or incompatible with `requirements`.
    #[must_use]
    pub fn fallbacks_for(
        &self,
        primary: &ModelDescriptor,
        requirements: &RequestRequirements,
    ) -> Vec<Arc<ModelDescriptor>> {
        let mut fallbacks = Vec::new();
        for raw in &primary.model().fallbacks {
            let Ok(spec) = raw.parse::<ModelSpec>() else {
                // Configuration validation rejects unparsable fallbacks; a stale
                // registry can only contain valid ones.
                continue;
            };
            if spec == primary.spec() {
                continue;
            }
            let Some(record) = self.providers.get(spec.provider()) else {
                continue;
            };
            if !record.enabled {
                continue;
            }
            let Some(descriptor) = self.models.get(&spec) else {
                continue;
            };
            if !descriptor.is_enabled() {
                continue;
            }
            if fallbacks
                .iter()
                .any(|existing: &Arc<ModelDescriptor>| existing.spec() == spec)
            {
                continue;
            }
            let mut candidate_requirements = *requirements;
            candidate_requirements =
                candidate_requirements.clamp_output_tokens(descriptor.capabilities());
            if check_capabilities(
                descriptor.provider_id(),
                descriptor.model_id(),
                descriptor.capabilities(),
                &candidate_requirements,
            )
            .is_err()
            {
                tracing::debug!(
                    provider = %descriptor.provider_id(),
                    model = %descriptor.model_id(),
                    "skipping fallback model that does not satisfy the request"
                );
                continue;
            }
            fallbacks.push(Arc::clone(descriptor));
        }
        fallbacks
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::adapters::MockAdapter;
    use crate::provider::config::{ModelCapabilities, ProviderProtocol, RetryPolicy};
    use crate::{AiCoreConfig, ConfigStore};

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig::new(
            ProviderId::new(id).unwrap(),
            ProviderProtocol::OpenAiCompatible,
        )
        .with_api_key("sk-test")
        .with_model(ModelConfig::new(format!("{id}-model")))
        .with_default_model(format!("{id}-model"))
    }

    fn registry() -> (ProviderRegistry, Arc<MockAdapter>) {
        let mut config = AiCoreConfig::default();
        config
            .providers
            .insert(ProviderId::new("a").unwrap(), provider("a"));
        let store = ConfigStore::new(config).unwrap();
        let snapshot = store.snapshot();
        let mut registry = ProviderRegistry::empty(snapshot.version());
        let mock = Arc::new(MockAdapter::new(ProviderId::new("a").unwrap()));
        registry.register(provider("a"), mock.clone()).unwrap();
        (registry, mock)
    }

    #[test]
    fn registers_and_resolves_models() {
        let (registry, _) = registry();
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.models().len(), 1);

        let resolved = registry.resolve(None).unwrap();
        assert_eq!(resolved.provider_id().as_str(), "a");
        assert_eq!(resolved.model_id(), "a-model");
        assert_eq!(resolved.spec().to_string(), "a/a-model");

        let explicit = registry
            .resolve(Some(&"a/a-model".parse::<ModelSpec>().unwrap()))
            .unwrap();
        assert_eq!(explicit.model_id(), "a-model");
    }

    #[test]
    fn unknown_or_disabled_targets_are_rejected() {
        let (mut registry, _) = registry();
        let unknown = registry
            .resolve(Some(&"a/missing".parse::<ModelSpec>().unwrap()))
            .unwrap_err();
        assert!(unknown.to_string().contains("missing"));

        let spec = "a/a-model".parse::<ModelSpec>().unwrap();
        registry.set_model_enabled(&spec, false).unwrap();
        assert!(registry.resolve(Some(&spec)).is_err());

        registry.set_model_enabled(&spec, true).unwrap();
        registry
            .set_provider_enabled(&ProviderId::new("a").unwrap(), false)
            .unwrap();
        assert!(registry.resolve(None).is_err());
    }

    #[test]
    fn empty_registry_reports_a_configuration_error() {
        let registry = ProviderRegistry::empty(ConfigVersion::new(1));
        assert!(registry.is_empty());
        assert!(registry.resolve(None).is_err());
    }

    #[test]
    fn protocol_mismatch_is_rejected() {
        let mut registry = ProviderRegistry::empty(ConfigVersion::new(1));
        let mut config = provider("a");
        config.protocol = ProviderProtocol::Anthropic;
        let mock = Arc::new(MockAdapter::new(ProviderId::new("a").unwrap()));
        assert!(registry.register(config, mock).is_err());
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let (mut registry, _) = registry();
        let mock = Arc::new(MockAdapter::new(ProviderId::new("a").unwrap()));
        assert!(registry.register(provider("a"), mock).is_err());
    }

    #[test]
    fn fallbacks_are_ordered_and_filtered() {
        let mut primary = provider("a");
        primary.models[0].fallbacks = vec!["b/b-model".to_owned(), "a/a-model".to_owned()];

        let mut config = AiCoreConfig::default();
        config
            .providers
            .insert(ProviderId::new("a").unwrap(), primary.clone());
        config
            .providers
            .insert(ProviderId::new("b").unwrap(), provider("b"));
        config.validate().unwrap();

        let mut registry = ProviderRegistry::empty(ConfigVersion::new(1));
        for id in ["a", "b"] {
            let provider_id = ProviderId::new(id).unwrap();
            let adapter = Arc::new(MockAdapter::new(provider_id.clone()));
            let config = config.providers.get(&provider_id).cloned().unwrap();
            registry.register(config, adapter).unwrap();
        }

        let requirements = RequestRequirements::from_request(
            &crate::provider::types::AiRequest::user_prompt("hi"),
            false,
        );
        let resolved = registry.resolve(None).unwrap();
        let fallbacks = registry.fallbacks_for(&resolved, &requirements);
        assert_eq!(fallbacks.len(), 1);
        assert_eq!(fallbacks[0].provider_id().as_str(), "b");

        // A fallback that cannot satisfy the request is skipped: require vision.
        let capabilities = ModelCapabilities {
            vision: true,
            ..ModelCapabilities::default()
        };
        let vision_requirements = RequestRequirements {
            streaming: false,
            tool_calling: false,
            structured_output: false,
            vision: true,
            estimated_input_tokens: 1,
            requested_output_tokens: 1,
        };
        assert!(registry
            .fallbacks_for(&resolved, &vision_requirements)
            .is_empty());
        assert!(capabilities.vision);
    }

    #[test]
    fn retry_policy_defaults_are_sane() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 2);
        policy.validate("retry").unwrap();
    }
}
