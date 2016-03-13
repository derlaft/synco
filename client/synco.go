package main

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// server.command* server.info*
// mpv.stdin mpv.stdout* mpv.stderr*
// stdin stdout* stderr*

// stdin -> mpv.stdin
// server.command -> mpv.stdin
// mpv.stderr -> server.info
// mpv.stderr -> stderr
// mpv.stdout -> stdout
// stdin -> mpv.stdin

const (
	MPLAYER = "mpv"
)

var (
	dir, _ = filepath.Abs(filepath.Dir(os.Args[0]))

	MPLAYER_ARGS = []string{
		"-input-file=/dev/stdin",
		"--no-input-terminal",
		"--script", path.Join(dir, "helper.lua"),
		"--term-status-msg", "STATUS ${=time-pos} ${=pause}",
	}
)

type message struct {
	action     string
	parameters []string
}

func (m *message) getPauseTime() (float64, error) {

	if len(m.parameters) < 1 {
		return 0.0, fmt.Errorf("Message `%s` has no parameters", m.action)
	}

	time, err := strconv.ParseFloat(m.parameters[0], 64)
	if err != nil {
		return 0.0, err
	}

	return time, nil
}

func (m *message) getBool() bool {
	return len(m.parameters) >= 1 && m.parameters[0] == "true"
}

var (
	lastPing time.Time
)

func healthCheck() {
	for {
		select {
		case <-time.After(time.Second * 2):
			if time.Since(lastPing) > time.Second {
				setoChanged(false)
			}
		}
	}
}

func (m *message) handle() error {

	switch m.action {
	case "seek":
		time, err := m.getPauseTime()
		if err != nil {
			return err
		}
		seekRecieved(time)
	case "seto":
		setoChanged(m.getBool())
	case "ping":
		lastPing = time.Now()
		sockOut <- "pong :3"
	default:
		return fmt.Errorf("Unknown message: `%v`", m)
	}

	return nil
}

func parseMessage(a string) (*message, error) {
	tokens := strings.Split(a, " ")

	if len(tokens) < 1 {
		return nil, fmt.Errorf("Got empty message")
	}

	return &message{
		action:     tokens[0],
		parameters: tokens[1:],
	}, nil
}

func statusParse(in <-chan string, out chan<- string) {
	for {
		select {
		case str := <-in:
			go func() {
				fmt.Println("Got str", str)
				tokens := strings.Split(str, " ")

				if len(tokens) < 1 || tokens[0] != "[helper]" {
					fmt.Println(str) // skipping non-helper string
					return
				}

				switch tokens[1] {
				case "ready":
					readyChanged(tokens[2] == "true")
				case "seek":
					position, _ := strconv.ParseFloat(tokens[2], 64)
					seekChanged(position)
				default:
					fmt.Fprintf(os.Stderr, "Unknown message type `%s`", tokens[1])
				}
			}()
		}
	}
}

var event = true

func seekChanged(time float64) {
	if !event {
		sockOut <- fmt.Sprintf("seek %v", time)
	}
	event = false
}

func seekRecieved(time float64) {
	event = true
	com <- fmt.Sprintf("seek %f absolute exact", time)
}

func setoChanged(seto bool) {
	com <- fmt.Sprintf("script-message seto %t", seto)
}

func readyChanged(ready bool) {
	sockOut <- fmt.Sprintf("ready %t", ready)
}

func listenCmd(a chan string) {
	for {
		select {
		case str := <-a:
			message, err := parseMessage(str)
			if err != nil {
				fmt.Fprintf(os.Stderr, "Got error while parsing message `%s`: %s\n", str, err)
				continue
			}

			err = message.handle()
			if err != nil {
				fmt.Fprintf(os.Stderr, "Got error while handling message `%s`: %s\n", str, err)
				continue
			}
		}
	}
}

var (
	statusIn    = make(chan string, 2)
	statusOut   = make(chan string, 2)
	com         = make(chan string, 2)
	sockIn      = make(chan string, 2)
	sockOut     = make(chan string, 2)
	statusPause = make(chan bool, 2)
)

func main() {

	if len(os.Args) != 3 {
		fmt.Fprintf(os.Stderr, "Usage: %s server_addr:server_port filename\n", os.Args[0])
		os.Exit(1)
	}

	addr, filename := os.Args[1], os.Args[2]
	cmd := exec.Command(MPLAYER, append(MPLAYER_ARGS, filename)...)

	mpvStdin, err1 := cmd.StdinPipe()
	mpvStdout, err2 := cmd.StdoutPipe()

	if err1 != nil || err2 != nil {
		fmt.Fprintf(os.Stderr, "Could not open pipe\n")
		os.Exit(2)
	}

	go connect(addr)

	cmd.Start()
	defer func() {
		cmd.Process.Kill()
	}()

	// stdin  -- commands
	go split(os.Stdin, com)
	go join(mpvStdin, com)

	// stderr -- status
	go split(mpvStdout, statusIn)
	go statusParse(statusIn, statusOut)
	go join(os.Stderr, statusOut)

	err := cmd.Wait()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Mplayer exited unsuccessfully: %s\n", err)
		os.Exit(2)
	}

}

func connect(addr string) {
	go listenCmd(sockIn)
	go healthCheck()

	for {
		conn, err := net.DialTimeout("tcp", addr, time.Second*10)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Could dial server: %v\n", err)
			<-time.After(time.Second)
			continue
		}

		go join(conn, sockOut)
		split(conn, sockIn)
	}
}

// Network stuff
func split(in io.Reader, out chan<- string) {
	scanner := bufio.NewScanner(in)
	for scanner.Scan() {
		out <- scanner.Text()
	}

	if err := scanner.Err(); err != nil {
		fmt.Fprintf(os.Stderr, "Pipe error: %v\n", err)
	}
}

func join(out io.Writer, in <-chan string) {
	for {
		select {
		case str := <-in:
			_, err := fmt.Fprintf(out, "%s\n", str)

			if err != nil {
				fmt.Fprintf(os.Stderr, "Pipe write error: %v\n", err)
				return
			}
		}
	}
}
