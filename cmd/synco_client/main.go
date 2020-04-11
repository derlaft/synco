package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"path"
	"sync"

	"github.com/derlaft/synco/protocol"
)

type synco struct {
	clientID       string
	socket         string
	clientPaused   bool
	clientReady    bool
	serverReady    bool
	ignoreSeek     int
	seekReceived   bool
	position       float64
	serverAddr     string
	mpvCommands    chan string
	serverCommands chan *protocol.Message
}

func create() *synco {

	hostname, err := os.Hostname()
	if err != nil {
		log.Fatalf("Error while receiving hostname: %v", err)
	}

	if sid := os.Getenv("SYNCO_ID"); sid != "" {
		hostname = sid
	}

	serverAddr := os.Getenv("SYNCO_SERVER")
	if serverAddr == "" {
		log.Fatalf("SYNCO_SERVER not set")
	}

	s := synco{
		clientID:       hostname,
		mpvCommands:    make(chan string, 64),
		serverCommands: make(chan *protocol.Message, 64),
		clientPaused:   true,
		clientReady:    false,
		serverReady:    false,
		position:       0,
		serverAddr:     serverAddr,
	}

	var runDir = os.Getenv("XDG_RUNTIME_DIR")
	if runDir == "" {
		runDir = "/tmp"
	}

	s.socket = path.Join(
		runDir,
		fmt.Sprintf("synco-%d.socket", os.Getpid()),
	)

	return &s
}

func (s *synco) cleanup() {
	err := os.Remove(s.socket)
	if err != nil {
		log.Printf("Warning: could not remove socket file: %v", err)
	}
}

func main() {

	s := create()

	defer s.cleanup()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	wg := sync.WaitGroup{}
	wg.Add(3)

	go func() {

		err := s.runMpv(ctx)
		if err != nil {
			log.Printf("Warning: mpv returned non-zero exit code: %v", err)
		}
		wg.Done()
		cancel()
	}()

	// mpv is slow to startup
	s.waitForMpvSocket()

	go func() {

		err := s.eventLoop(ctx)
		if err != nil {
			log.Printf("Warning: mpv ipc client aborted: %v", err)
		}
		wg.Done()
		cancel()
	}()

	go func() {

		err := s.dialServer(ctx)
		if err != nil {
			log.Printf("Warning: server client aborted: %v", err)
		}
		wg.Done()
		cancel()
	}()

	wg.Wait()

}

func (s *synco) pauseChanged(paused bool) {

	if !s.clientPaused && paused {

		s.clientReady = false
		s.serverCommands <- &protocol.Message{
			ID: protocol.ClientUnreadyMessage,
		}

	}

	// client wants to unpause, server is not ready
	if !paused && !s.serverReady {
		s.mpvCommands <- pauseCommand
		s.mpvCommands <- othersNotReady
	}

	s.clientPaused = paused

}

func (s *synco) readyPressed() {
	s.clientReady = !s.clientReady
	if s.clientReady {
		s.mpvCommands <- waitingForOthers
		s.serverCommands <- &protocol.Message{
			ID: protocol.ClientReadyMessage,
		}
	} else {
		s.mpvCommands <- blockingOthers
		s.serverCommands <- &protocol.Message{
			ID: protocol.ClientUnreadyMessage,
		}
	}
}

func (s *synco) positionChanged(pos float64) {

	if pos < 0 {
		pos = 0
	}

	if s.seekReceived {

		log.Printf("Local seek to %v", pos)

		s.serverCommands <- &protocol.Message{
			ID:       protocol.ClientSeekMessage,
			Position: pos,
		}

		s.seekReceived = false
	} else {

		s.serverCommands <- &protocol.Message{
			ID:       protocol.ClientPositionMessage,
			Position: pos,
		}

	}

	s.position = pos
}

func (s *synco) onServerReady(ready bool) {
	s.serverReady = ready
	if ready {
		s.mpvCommands <- unpauseCommand
	} else {
		s.mpvCommands <- pauseCommand
	}
}

func (s *synco) remoteSeek(pos float64) {
	s.ignoreSeek++
	s.mpvCommands <- fmt.Sprintf(seekCommand, pos)
}
