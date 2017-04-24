# synco

Watch anime (or anything else!) together with friends over network!

Usage:

1. Download sources and build (you will need mpv (mplayer is fine too, but why would you still use that in 2017?), go, and [gb](https://getgb.io/):

```bash
git clone https://github.com/derlaft/synco.git
cd synco
gb build
```

2. Run server

```bash
./bin/server 0.0.0.0:4042
```

3. Setup client. Place this to ``~/.config/synco``:

```
[Client]
ID = der # your unique ID
ConnectTo = serveraddr:4042
HelperDir = /home/user/synco/src/client/ # this is where helper.lua is located
```

4. Run it!

```bash
./bin/client ~/Downloads/My-Favourite-Animu/Season1/Episodo1.mkv
```

5. Press F1 to change your readiness. Once everybody is ready playback will be started.
