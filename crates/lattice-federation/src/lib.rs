mod error;
mod planner;

pub use error::FederationError;
pub use planner::FederationPlanner;

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SUPERGRAPH: &str = r#"
        schema
            @link(url: "https://specs.apollo.dev/link/v1.0")
            @link(url: "https://specs.apollo.dev/join/v0.3", for: EXECUTION)
        {
            query: Query
        }

        directive @join__graph(name: String!, url: String!) on ENUM_VALUE
        directive @join__type(graph: join__Graph!, key: join__FieldSet) repeatable on OBJECT | INTERFACE
        directive @join__field(graph: join__Graph) on FIELD_DEFINITION
        directive @link(url: String, as: String, for: link__Purpose, import: [link__Import]) repeatable on SCHEMA

        scalar join__FieldSet
        scalar link__Import

        enum link__Purpose {
            SECURITY
            EXECUTION
        }

        enum join__Graph {
            USERS @join__graph(name: "users", url: "http://users:3000/graphql")
            PRODUCTS @join__graph(name: "products", url: "http://products:3001/graphql")
        }

        type Query {
            me: User @join__field(graph: USERS)
            products: [Product] @join__field(graph: PRODUCTS)
        }

        type User @join__type(graph: USERS, key: "id") {
            id: ID!
            name: String
        }

        type Product @join__type(graph: PRODUCTS, key: "id") {
            id: ID!
            title: String
        }
    "#;

    #[test]
    fn load_supergraph() {
        let planner = FederationPlanner::from_sdl(TEST_SUPERGRAPH);
        assert!(planner.is_ok());
    }

    #[test]
    fn rejects_invalid_sdl() {
        let planner = FederationPlanner::from_sdl("not valid graphql {{{}}}");
        assert!(planner.is_err());
    }
}
