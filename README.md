# Dotpong

Pushes timing metrics of Substrate chains to an [Instatus](https://instatus.com/) page via REST API. For this to work you will need to create an account on that website and get an API key.

## Usage

Create an `.env` file with the following keys:
```pre
INSTATUS_KEY=
SUBSTRATE_URI=""
```

Modify the `config.json` with your metric and page IDs:

```json
{
	"page": "..",
	"interval_sec": 600,
	"transactions": [
		{
			"rpc": "wss://<my network rpc>",
			"metrics": {
				"inclusion": "..",
				"finalization": ".."
			}
		},
	]
}
```

The `metrics.py` script can be used to print all metric ids for your Page. It therefore needs the page-id in the config.

Start it with: `cargo run --release`. Maybe you want to put that in an infinite loop, since its not 100% crash resistant.

## License

GPL-3.0 only, see [LICENSE](LICENSE). (C) Oliver Tale-Yazdi.
