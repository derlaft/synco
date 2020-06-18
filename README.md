# synco

Watch anime (or anything else!) together with friends over network!

Usage:

1. Install `go`, setup `GOBIN` environment variable. You will need mpv (mplayer is fine too, but why would you still use that in 2017?). Alternatively, you could use [gobin](https://github.com/myitcv/gobin) to install the package.

```bash
go get github.com/derlaft/synco/cmd/synco_server
go get github.com/derlaft/synco/cmd/synco_client
```

2. Run server.

```bash
synco_server -listen 0.0.0.0:4042
```

3. Setup client. Configuration is done via environment variables:

* `SYNCO_ID` is a unique client ID
* `SYNCO_SERVER` is server addr

```
SYNCO_ID=der SYNCO_SERVER=example.com:4042 synco_client filename.mpv
```

5. Press F1 to change your readiness. Once everybody is ready playback will be started.


# Known issues

* On certain files it is not possible to scroll precisely. Currently this files make synco a little bit crazy.
