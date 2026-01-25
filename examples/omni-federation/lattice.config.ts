import { config, graph } from '@grafbase/sdk'

export default config({
  graph: graph.Federated({
    subgraphs: [
      {
        name: 'aether',
        url: process.env.AETHER_GRAPHQL_URL || 'http://localhost:4001/graphql',
      },
      {
        name: 'warden',
        url: process.env.WARDEN_GRAPHQL_URL || 'http://localhost:4002/graphql',
      },
    ],
  }),
})
