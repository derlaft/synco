package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"math"
	"net"
	"sync"
	"time"

	"github.com/derlaft/synco/protocol"
)

type server struct {
	clients  map[string]*client
	listener net.Listener
	ready    bool
	sync.RWMutex
	maxDesync float64
}

type client struct {
	conn   net.Conn
	writer *json.Encoder
	id     string
	ready  bool
	pos    float64
}

func main() {

	listenAddr := flag.String("listen", "0.0.0.0:4042", "Address to listen on")
	maxDesync := flag.Float64("max-desync", 5.0, "Maximum lag between clients (seconds)")

	flag.Parse()

	log.Printf("Starting server on %v", *listenAddr)

	l, err := net.Listen("tcp4", *listenAddr)
	if err != nil {
		log.Fatalf("Error while binding to %v: %v", *listenAddr, err)
	}

	defer func() {
		err := l.Close()
		if err != nil {
			log.Printf("Warning: error while closing listener: %v", err)
		}
	}()

	s := &server{
		listener:  l,
		clients:   make(map[string]*client),
		maxDesync: *maxDesync,
	}

	err = s.acceptLoop()
	if err != nil {
		log.Fatalf("Error while processing new clients: %v", err)
	}

}

func (s *server) acceptLoop() error {

	go func() {

		for range time.Tick(time.Second) {
			s.onPing()
		}

	}()

	for {

		// wait for accept
		c, err := s.listener.Accept()
		if err != nil {
			return err
		}

		// handle client
		go func() {
			err := s.handleClient(c)
			if err != nil {
				log.Printf("Error while handling client: %v", err)
			}
		}()
	}
}

func (s *server) onReadyChange() {
	s.RLock()
	defer s.RUnlock()

	var globalReady = false

	if len(s.clients) > 1 {
		var everyoneReady = true

		for _, c := range s.clients {
			everyoneReady = everyoneReady && c.ready
		}

		globalReady = everyoneReady
	}

	if globalReady && !s.ready {

		log.Println("Everyone is ready")

		s.broadcast("", protocol.Message{
			ID: protocol.ServerReadyMessage,
		})

		s.ready = true

		return
	}

	if !globalReady && s.ready {

		log.Println("Someone is now not ready")

		s.broadcast("", protocol.Message{
			ID: protocol.ServerUnreadyMessage,
		})

		s.ready = false

	}
}

func (s *server) onSeek(from string, to float64) {
	s.broadcast(from, protocol.Message{
		ID:       protocol.ServerSeekMessage,
		Position: to,
	})
}

func (s *server) onPing() {
	s.RLock()
	defer s.RUnlock()

	for leftID, leftC := range s.clients {
		for rightID, rightC := range s.clients {

			if leftID == rightID {
				continue
			}

			if desync := math.Abs(leftC.pos - rightC.pos); desync > s.maxDesync {
				// desync detected

				log.Printf("De-desync detected between %v and %v: %vs", leftID, rightID, desync)

				s.broadcast("", protocol.Message{
					ID: protocol.ServerUnreadyMessage,
				})

				return
			}
		}
	}
}

func (s *server) broadcast(from string, msg protocol.Message) {
	s.RLock()
	defer s.RUnlock()

	for cid, c := range s.clients {

		if cid == from {
			continue
		}

		go func(cid string, c *client) {

			err := c.writer.Encode(msg)
			if err != nil {
				log.Printf("Warning: error while broadcasting to %v: %v", cid, err)
			}

		}(cid, c)
	}
}

func (s *server) handleClient(conn net.Conn) error {

	defer func() {
		err := conn.Close()
		if err != nil {
			log.Printf("Warning: error while closing client: %v", err)
		}
	}()

	reader := json.NewDecoder(conn)
	writer := json.NewEncoder(conn)

	var c = client{
		conn:   conn,
		writer: writer,
	}

	var helloMessage protocol.Message
	err := reader.Decode(&helloMessage)
	if err != nil {
		return err
	}

	if helloMessage.ID != protocol.HelloMessage {
		return fmt.Errorf("Expected HelloMessage, got %v", helloMessage.ID)
	}

	c.id = helloMessage.UserID

	// if there's another client with this ID - kick him
	s.Lock()
	if oldClient, ok := s.clients[c.id]; ok {
		log.Printf("Warning: kicking old client with id=%v", c.id)
		err = oldClient.conn.Close()
		if err != nil {
			log.Printf("Warning: error while closing client: %v", err)
		}
	}
	s.clients[c.id] = &c
	s.Unlock()

	// stop playback
	s.onReadyChange()

	// reader loop
	for reader.More() {

		// read msg
		var msg protocol.Message
		err := reader.Decode(&msg)
		if err != nil {
			return err
		}

		// process msg
		switch msg.ID {
		case protocol.ClientReadyMessage:
			log.Printf("Client %v is ready", c.id)
			c.ready = true
			s.onReadyChange()
		case protocol.ClientUnreadyMessage:
			log.Printf("Client %v is not ready", c.id)
			c.ready = false
			s.onReadyChange()
		case protocol.ClientPositionMessage:
			c.pos = msg.Position
		case protocol.ClientSeekMessage:
			log.Printf("Client %v seeks", c.id)
			c.pos = msg.Position
			s.onSeek(c.id, c.pos)
		}

	}

	return nil
}
