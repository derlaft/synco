package main

import (
	"fmt"
	"gopkg.in/gcfg.v1"
	"log"
	"os"
	"path"
	"path/filepath"
	"protocol"
	"strconv"
	"strings"
)

var (
	ctl *mpv
	net *network
)

type config struct {
	Client struct {
		ID        string
		ConnectTo string
		HelperDir string
	}
}

func handle(msg *protocol.Event) error {

	switch {
	case msg.Time != nil:
		seekRecieved(msg.Time.Where)
	case msg.Go != nil:
		goRecieved(msg.Go.Playback)
	default:
		return fmt.Errorf("Unknown message: `%v`", msg)
	}

	return nil
}

func stahp() {
	goRecieved(false)
}

func goRecieved(play bool) {
	ctl.execCmd(fmt.Sprintf("script-message seto %t", play))
}

func parse() {
	defer log.Println("stopped parse proc: should only happen on mpv termination")
	for {
		str, eof := ctl.event()
		if eof {
			log.Println("eof")
			break
		}

		tokens := strings.Split(str, " ")

		if len(tokens) < 1 || tokens[0] != "[helper]" {
			continue
		}

		switch tokens[1] {
		case "ready":
			net.setReady(tokens[2] == "true", "origin: mpv helper")
		case "seek":
			position, _ := strconv.ParseFloat(tokens[2], 64)
			seekChanged(position)
		default:
			fmt.Fprintf(os.Stderr, "Unknown message type `%s`\n", tokens[1])
		}
	}
}

var event = true

func seekChanged(time float64) {
	if !event {
		net.seek(time, "origin: mpv helper")
	}
	event = false
}

func seekRecieved(time float64) {
	log.Printf("Seek recieved -- %v, chaning time", time)
	event = true
	ctl.execCmd(fmt.Sprintf("seek %f absolute exact", time))
}

func main() {

	if len(os.Args) < 2 {
		fmt.Fprintf(os.Stderr, "Usage: %s filename [configfile]\n", os.Args[0])
		os.Exit(1)
	}

	var configLocation = path.Join(os.Getenv("HOME"), ".config", "synco")
	if len(os.Args) == 3 {
		configLocation = os.Args[2]
	}

	cfg := config{}
	err := gcfg.ReadFileInto(&cfg, configLocation)
	if err != nil {
		log.Print(err)
		return
	}

	if cfg.Client.HelperDir <= "" {
		cfg.Client.HelperDir, _ = filepath.Abs(filepath.Dir(os.Args[0]))
	}
	helperPath := path.Join(cfg.Client.HelperDir, "helper.lua")
	if _, err := os.Stat(helperPath); os.IsNotExist(err) {
		log.Printf("helper file (%v) not found", helperPath)
		return
	}

	mplayerArgs := []string{
		"-input-file=/dev/stdin",
		"--no-input-terminal",
		"--script", path.Join(cfg.Client.HelperDir, "helper.lua"),
		"--term-status-msg", "STATUS ${=time-pos} ${=pause}",
		os.Args[1],
	}

	// connect to server && say "hello"
	connection, err := connect(cfg.Client.ConnectTo, cfg.Client.ID, handle, stahp)
	if err != nil {
		log.Print(err)
		return
	}
	net = connection
	go net.healthPings()

	process, err := start(mplayerArgs)
	if err != nil {
		log.Fatal(err)
	}

	ctl = process
	defer ctl.stop()

	// mpv output parsing
	go parse()

	ctl.wait()
}
