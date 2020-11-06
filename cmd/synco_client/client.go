package main

import (
	"context"
	"encoding/json"
	"log"
	"net"

	"github.com/derlaft/synco/protocol"
)

func (s *synco) dialServer(ctx context.Context) error {

	rpc, err := net.Dial("tcp", s.serverAddr)
	if err != nil {
		return err
	}

	go func() {
		<-ctx.Done()

		close(s.serverCommands)
		log.Printf("Stopping server client")
		err = rpc.Close()
		if err != nil {
			log.Printf("Warning: could not close ipc client conn: %v", err)
		}
	}()

	go func() {

		encoder := json.NewEncoder(rpc)

		for command := range s.serverCommands {
			if command == nil {
				log.Printf("Client command writing terminating")
				return
			}

			err = encoder.Encode(command)
			if err != nil {
				log.Printf("Error while writing client command: %v", err)
				return
			}
		}
	}()

	s.serverCommands <- &protocol.Message{
		ID:     protocol.HelloMessage,
		UserID: s.clientID,
	}

	decoder := json.NewDecoder(rpc)
	for decoder.More() {

		var resp protocol.Message
		err := decoder.Decode(&resp)
		if err != nil {
			return err
		}

		log.Printf("New server msg: %+v", resp)

		switch resp.ID {

		case protocol.ServerReadyMessage, protocol.ServerUnreadyMessage:
			s.onServerReady(resp.ID == protocol.ServerReadyMessage)
		case protocol.ServerSeekMessage:
			s.remoteSeek(resp.Position)
		case protocol.ServerSpeedMessage:
			s.remoteSpeed(resp.Speed)
		default:
			log.Printf("Warning: unknown server msg type: %+v", resp)
		}
	}

	return nil
}
