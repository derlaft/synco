package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"os"
	"os/exec"
	"time"
)

func (s *synco) waitForMpvSocket() {

	maxTries := 100
	for maxTries > 0 {
		_, err := os.Stat(s.socket)
		if err != nil {

			if os.IsNotExist(err) {
				time.Sleep(time.Second / 10)
				maxTries--
				continue
			}

			log.Fatalf("Error while trying to open socket file: %v", err)
		}

		return
	}

	log.Fatalf("Waited for mpv socket to appear, but it did not")
}

func (s *synco) runMpv(ctx context.Context) error {

	args := append([]string{
		"--input-ipc-server=" + s.socket,
		"--pause",
	}, os.Args[1:]...)

	// nolint:gosec
	cmd := exec.Command("mpv", args...)

	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout

	go func() {
		<-ctx.Done()
		log.Printf("Killing mpv process")
		err := cmd.Process.Kill()
		if err != nil {
			log.Printf("Warning: could not kill mpv process: %v", err)
		}
	}()

	err := cmd.Run()
	if err != nil {
		return err
	}

	return nil
}

// IPCResponse represents some stuff that mpv sends to us
type IPCResponse struct {
	Event string        `json:"event"`
	Error string        `json:"error"`
	Name  string        `json:"name"`
	Data  interface{}   `json:"data"`
	Args  []interface{} `json:"args"`
}

func (s *synco) eventLoop(ctx context.Context) error {

	rpc, err := net.Dial("unix", s.socket)
	if err != nil {
		return err
	}

	go func() {
		<-ctx.Done()
		log.Printf("Stopping IPC client")
		close(s.mpvCommands)
		err = rpc.Close()
		if err != nil {
			log.Printf("Warning: could not close ipc client conn: %v", err)
		}
	}()

	go func() {
		for command := range s.mpvCommands {
			if command == "" {
				log.Printf("Command writing terminating")
				return
			}

			_, err = rpc.Write([]byte(command + "\n"))
			if err != nil {
				log.Printf("Error while writing the command: %v", err)
				return
			}

		}
	}()

	// write initialization sequence
	for _, cmd := range initSeq {
		s.mpvCommands <- cmd
	}

	decoder := json.NewDecoder(rpc)
	for decoder.More() {

		var resp IPCResponse
		err := decoder.Decode(&resp)
		if err != nil {
			return err
		}

		switch {

		// handle non-success responses to our commands
		case resp.Error != "" && resp.Error != "success":
			return fmt.Errorf("IPC command failed: %+v", resp)

			// handle success responses to our commands
		case resp.Error != "" && resp.Error == "success":

		// pause/unpause handling
		case resp.Event == "pause":
			s.pauseChanged(true)

		case resp.Event == "unpause":
			s.pauseChanged(false)

		case resp.Event == "playback-restart":
			s.seekReceived = true

		// ready button handling
		case resp.Event == "client-message":

			if len(resp.Args) != 1 {
				return fmt.Errorf("IPC response too many arguments: %+v", resp)
			}

			messageID, ok := resp.Args[0].(string)
			if !ok {
				return fmt.Errorf("IPC response arg (%T) is not str: %+v", resp.Args[0], resp)
			}

			if messageID == "ready_pressed" {
				s.readyPressed()
			}

		// position tracking handling
		case resp.Event == "property-change" && resp.Name == "time-pos":

			if resp.Data == nil {
				// happens on startup
				continue
			}

			pos, ok := resp.Data.(float64)
			if !ok {
				return fmt.Errorf("IPC response data (%T) is not str: %+v", resp.Data, resp)
			}

			s.positionChanged(pos)

		default:
			log.Printf("new unhandled message: %+v", resp)
		}

	}

	return nil
}
