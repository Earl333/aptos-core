# Aptos Node Health Checker
The Aptos Node Health Checker (NHC) is the reference implementation of a node health checker for Validator Nodes (Validators), Validator FullNodes (VFNs), and Public FullNodes (PFNs). The node health checker aims to serve 3 major user types:
- **AIT Registration**: As part of sign up for the Aptos Incentivized Testnets (AIT), we request that users demonstrate that they can run a ValidatorNode successfully. We use this tool to encode precisely what that means.
- **Operator Support**: As node operators, you will want to know whether your node is running correctly. This service can help you figure that out. While we host our own instances of this service, we encourage node operators to run their own instances. You may choose to either run a publicly available NHC or run it as a sidecar, where it only works against your own node.
- **Continuous Evaluation**: As part of the AITs, Aptos Labs needs a tool to help confirm that participants are running their nodes in a way that meets our criteria. We run this tool continuously throughout each AIT to help us evaluate this.

In this README we describe how to run NHC in each of the above configuration.

## Running NHC
For now, the best way to run NHC is to build and run it locally.

First, check out the repo and navigate here.
```
git clone git@github.com:aptos-labs/aptos-core.git
cd aptos-core/ecosystem/node-checker
```

Next, generate a baseline configuration. In this example we generate a configuration for a FullNode on the devnet, running on a machine in the local network:
```
cargo run -- configuration create --configuration-name "My Server Devnet Fullnode" --url http://192.168.86.2 --evaluators state_sync -o /tmp/my_server_devnet_fullnode.yaml
```

Finally, run NHC:
```
cargo run -- server run --baseline-node-config-paths /tmp/my_server_devnet_fullnode.yaml
```
Where `--baseline-node-url` is the node that will be used as the baseline against which NHC will compare the test node (the node under investigation).

Instructions for running the tool using Docker / Terraform are coming soon.

## Developing
To develop this app, you should first run two nodes of the same type. See [this wiki](https://aptos.dev/tutorials/full-node/run-a-fullnode) for guidance on how to do this. You may also target a known existing FullNode with its metrics port open.

The below command assumes we have a fullnode running locally, the target node (the node under investigation), and another running on a machine in our network, the baseline node (the node we compare the target to):
```
cargo run -- -d --baseline-node-url 'http://192.168.86.2' --target-node-url http://localhost --evaluators state_sync --allow-preconfigured-test-node-only
```
This runs NHC in sidecar mode, where only the `/check_preconfigured_node` endpoint can be called, which will target the node running on localhost.

Once the service is running, you can query it like this:
```
$ curl -s localhost:20121/api/check_preconfigured_node | jq .
{
  "evaluations": [
    {
      "headline": "State sync version is within tolerance",
      "score": 100,
      "explanation": "Successfully pulled metrics from target node twice, saw the version was progressing, and saw that it is within tolerance of the baseline node. Target version: 1882004. Baseline version: 549003. Tolerance: 1000"
    }
  ],
  "summary_score": 100,
  "summary_explanation": "100: Awesome!"
}
```

## Generating OpenAPI spec
First run an instance of the service following the instructions above (any of them should work, the API doesn't change between configurations). From then, you can retrieve the spec like this:
```
curl localhost:20121/spec_yaml > openapi.yaml
echo '' >> openapi.json
curl localhost:20121/spec_json > openapi.json
```
For now the openapi specs in this repo were generated by doing the above. I'll set up CI soon to do this for me so we can't forget to update it.

## Development notes
I will remove these notes once I get this service out of deep development.

### Dependencies
While relying on my branch of poem, if you ever update that, run this from the repo root to update the dep locally:
```
cargo update -p poem -p poem-openapi
```

### Confirming that the preconfiguration of target node works
These should be turned into actual integration tests.

```
$ cargo run -- -d --baseline-node-url 'http://fullnode.devnet.aptoslabs.com/' --target-node-url http://localhost --allow-preconfigured-test-node-only
$ curl -s -I localhost:20121/api/check_node | head -n 1
HTTP/1.1 405 Method Not Allowed
$ curl -s -I localhost:20121/api/check_preconfigured_node | head -n 1
HTTP/1.1 200 OK
```

```
$ cargo run -- -d --baseline-node-url 'http://fullnode.devnet.aptoslabs.com/'
$ curl -s -I localhost:20121/api/check_node | head -n 1
HTTP/1.1 200 OK
$ curl -s -I localhost:20121/api/check_preconfigured_node | head -n 1
HTTP/1.1 405 Method Not Allowed
```