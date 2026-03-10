mod error;
mod planner;

pub use error::FederationError;
pub use hive_router_query_planner::planner::plan_nodes::{ConditionNode, FetchNode, FlattenNode, PlanNode, QueryPlan};
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

        directive @join__enumValue(graph: join__Graph!) repeatable on ENUM_VALUE

        directive @join__field(
            graph: join__Graph
            requires: join__FieldSet
            provides: join__FieldSet
            type: String
            external: Boolean
            override: String
            usedOverridden: Boolean
        ) repeatable on FIELD_DEFINITION | INPUT_FIELD_DEFINITION

        directive @join__graph(name: String!, url: String!) on ENUM_VALUE

        directive @join__implements(
            graph: join__Graph!
            interface: String!
        ) repeatable on OBJECT | INTERFACE

        directive @join__type(
            graph: join__Graph!
            key: join__FieldSet
            extension: Boolean! = false
            resolvable: Boolean! = true
            isInterfaceObject: Boolean! = false
        ) repeatable on OBJECT | INTERFACE | UNION | ENUM | INPUT_OBJECT | SCALAR

        directive @join__unionMember(
            graph: join__Graph!
            member: String!
        ) repeatable on UNION

        directive @link(
            url: String
            as: String
            for: link__Purpose
            import: [link__Import]
        ) repeatable on SCHEMA

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

        type Query
            @join__type(graph: USERS)
            @join__type(graph: PRODUCTS)
        {
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

    #[test]
    fn plans_simple_query() {
        let planner = FederationPlanner::from_sdl(TEST_SUPERGRAPH).unwrap();
        let result = planner.plan("{ me { id name } }", None);
        assert!(result.is_ok(), "planning failed: {:?}", result.err());
        let plan = result.unwrap();
        assert!(plan.node.is_some());
    }

    #[test]
    fn plan_returns_error_for_invalid_query() {
        let planner = FederationPlanner::from_sdl(TEST_SUPERGRAPH).unwrap();
        let result = planner.plan("not a valid query {{{", None);
        assert!(result.is_err());
    }

    #[test]
    fn plan_returns_error_for_nonexistent_field() {
        let planner = FederationPlanner::from_sdl(TEST_SUPERGRAPH).unwrap();
        let result = planner.plan("{ nonExistentField }", None);
        assert!(result.is_err());
    }
}
