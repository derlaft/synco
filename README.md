# synco

Watch anime (or anything else!) together with friends over network!

Usage:

1. Setup go and `GOPATH` and probably `GOBIN` environment variables. You will need mpv (mplayer is fine too, but why would you still use that in 2017?).

```bash
go get github.com/derlaft/synco/synco_server
go get github.com/derlaft/synco/synco_client
```

2. Run server.

```bash
synco_server 0.0.0.0:4042
```

3. Setup client. Place this to ``~/.config/synco``.

```
[Client]
ID = der # your unique ID
ConnectTo = serveraddr:4042
HelperDir = /home/user/synco/src/client/ # this is where helper.lua is located
```

4. Run it!

```bash
synco_client ~/Downloads/My-Favourite-Animu/Season1/Episodo1.mkv
```

5. Press F1 to change your readiness. Once everybody is ready playback will be started.
