use std::collections::HashMap;

use lattice_config::ContractsConfig;
use lattice_federation::FederationPlanner;

/// Pre-computed contract variants.
pub struct ContractRegistry {
    variants: HashMap<String, ContractVariant>,
    default_key: String,
    header_name: String,
}

/// A single contract variant with its filtered SDL and planner.
pub struct ContractVariant {
    pub planner: FederationPlanner,
    pub sdl: String,
}

impl ContractRegistry {
    /// Build a registry from config and the raw supergraph SDL.
    ///
    /// Create the "full" variant (unfiltered) plus one variant per config entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the SDL cannot be parsed or filtered.
    pub fn build(config: &ContractsConfig, sdl: &str) -> Result<Self, lattice_federation::FederationError> {
        let mut variants = HashMap::new();

        // Always create the "full" (unfiltered) variant.
        let full_planner = FederationPlanner::from_sdl(sdl)?;
        variants.insert(
            "full".into(),
            ContractVariant {
                planner: full_planner,
                sdl: sdl.to_owned(),
            },
        );

        // Create configured variants.
        for (name, variant_config) in &config.variants {
            let filtered = lattice_federation::filter_sdl(sdl, &variant_config.include, &variant_config.exclude)?;
            let planner = FederationPlanner::from_sdl(&filtered)?;
            variants.insert(name.clone(), ContractVariant { planner, sdl: filtered });
        }

        Ok(Self {
            variants,
            default_key: config.default.clone(),
            header_name: config.header_name.clone(),
        })
    }

    /// Get a variant by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ContractVariant> {
        self.variants.get(key)
    }

    /// Return the default contract key.
    #[must_use]
    pub fn default_key(&self) -> &str {
        &self.default_key
    }

    /// Return the header name for contract selection.
    #[must_use]
    pub fn header_name(&self) -> &str {
        &self.header_name
    }

    /// Number of variants (including "full").
    #[must_use]
    pub fn variant_count(&self) -> usize {
        self.variants.len()
    }

    /// Resolve the contract key from available sources.
    ///
    /// Priority: hook result > JWT claim > header > default.
    #[must_use]
    pub fn resolve_key<'a>(
        &'a self,
        hook_contract: Option<&'a str>,
        claim_contract: Option<&'a str>,
        header_contract: Option<&'a str>,
    ) -> &'a str {
        hook_contract
            .or(claim_contract)
            .or(header_contract)
            .unwrap_or(&self.default_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGGED_SUPERGRAPH: &str = r#"
        schema
            @link(url: "https://specs.apollo.dev/link/v1.0")
            @link(url: "https://specs.apollo.dev/join/v0.3", for: EXECUTION)
        {
            query: Query
        }

        directive @tag(name: String!) repeatable on FIELD_DEFINITION | OBJECT
        directive @join__enumValue(graph: join__Graph!) repeatable on ENUM_VALUE
        directive @join__field(graph: join__Graph, requires: join__FieldSet, provides: join__FieldSet, type: String, external: Boolean, override: String, usedOverridden: Boolean) repeatable on FIELD_DEFINITION | INPUT_FIELD_DEFINITION
        directive @join__graph(name: String!, url: String!) on ENUM_VALUE
        directive @join__implements(graph: join__Graph!, interface: String!) repeatable on OBJECT | INTERFACE
        directive @join__type(graph: join__Graph!, key: join__FieldSet, extension: Boolean! = false, resolvable: Boolean! = true, isInterfaceObject: Boolean! = false) repeatable on OBJECT | INTERFACE | UNION | ENUM | INPUT_OBJECT | SCALAR
        directive @join__unionMember(graph: join__Graph!, member: String!) repeatable on UNION
        directive @link(url: String, as: String, for: link__Purpose, import: [link__Import]) repeatable on SCHEMA

        scalar join__FieldSet
        scalar link__Import
        enum link__Purpose { SECURITY EXECUTION }
        enum join__Graph {
            USERS @join__graph(name: "users", url: "http://users:3000/graphql")
        }

        type Query @join__type(graph: USERS) {
            me: User @join__field(graph: USERS)
            admin: String @join__field(graph: USERS) @tag(name: "internal")
        }

        type User @join__type(graph: USERS, key: "id") {
            id: ID!
            name: String
            email: String @tag(name: "internal")
        }
    "#;

    #[test]
    fn build_registry_creates_variants() {
        let mut variants = HashMap::new();
        variants.insert(
            "public".into(),
            lattice_config::ContractVariantConfig {
                include: vec![],
                exclude: vec!["internal".into()],
            },
        );

        let config = ContractsConfig {
            default: "full".into(),
            header_name: "X-Contract-Key".into(),
            variants,
        };

        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert!(registry.get("public").is_some());
        assert!(registry.get("full").is_some());
        assert!(registry.get("nonexistent").is_none());

        let public = registry.get("public").unwrap();
        assert!(!public.sdl.contains("email"), "public variant should not have email");
        assert!(public.sdl.contains("name"), "public variant should have name");

        let full = registry.get("full").unwrap();
        assert!(full.sdl.contains("email"), "full variant should have email");
    }

    #[test]
    fn default_key_and_header() {
        let config = ContractsConfig {
            default: "public".into(),
            header_name: "X-My-Contract".into(),
            variants: HashMap::new(),
        };

        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert_eq!(registry.default_key(), "public");
        assert_eq!(registry.header_name(), "X-My-Contract");
    }

    #[test]
    fn resolve_from_header() {
        let config = ContractsConfig {
            default: "full".into(),
            header_name: "X-Contract-Key".into(),
            variants: HashMap::new(),
        };
        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert_eq!(registry.resolve_key(None, None, Some("public")), "public");
    }

    #[test]
    fn resolve_from_claim() {
        let config = ContractsConfig {
            default: "full".into(),
            header_name: "X-Contract-Key".into(),
            variants: HashMap::new(),
        };
        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert_eq!(registry.resolve_key(None, Some("internal"), None), "internal");
    }

    #[test]
    fn resolve_falls_back_to_default() {
        let config = ContractsConfig {
            default: "full".into(),
            header_name: "X-Contract-Key".into(),
            variants: HashMap::new(),
        };
        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert_eq!(registry.resolve_key(None, None, None), "full");
    }

    #[test]
    fn variant_count_includes_full() {
        let mut variants = HashMap::new();
        variants.insert(
            "public".into(),
            lattice_config::ContractVariantConfig {
                include: vec![],
                exclude: vec!["internal".into()],
            },
        );

        let config = ContractsConfig {
            default: "full".into(),
            header_name: "X-Contract-Key".into(),
            variants,
        };

        let registry = ContractRegistry::build(&config, TAGGED_SUPERGRAPH).unwrap();
        assert_eq!(registry.variant_count(), 2); // "full" + "public"
    }
}
