package main

import (
	"fmt"
	"golang.org/x/net/context"
	"google.golang.org/grpc"
	"log"
	"math/rand"
	"protocol"
	"time"
)

//DefaultContext returns context with default timeout
func defaultCtx() (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.Background(), time.Millisecond*300)
}

type handlerFunc func(*protocol.Event) error
type stahpFunc func()

type network struct {
	id       string
	conn     *grpc.ClientConn
	client   protocol.SyncoClient
	stream   protocol.Synco_ConnectClient
	lastPong time.Time

	handler handlerFunc
	stahp   stahpFunc
}

func connect(addr, id string, handler handlerFunc, stahp stahpFunc) (*network, error) {
	conn, err := grpc.Dial(addr, grpc.WithInsecure())
	if err != nil {
		log.Fatal(err)
	}

	client := protocol.NewSyncoClient(conn)

	net := &network{
		id:      id,
		conn:    conn,
		client:  client,
		handler: handler,
		stahp:   stahp,
	}

	go net.doStreaming()
	return net, nil
}

func (n *network) doStreaming() {
	for {
		n.stream = nil
		stream, err := n.client.Connect(context.Background())
		if err != nil {
			log.Printf("Could not establish stream: %v", err)
			continue
		}
		err = stream.Send(&protocol.Event{
			Hello: &protocol.HelloEvent{
				Id: n.id,
			},
		})
		if err != nil {
			log.Printf("Could welcome the stream: %v", err)
			continue
		}
		n.stream = stream

		for {
			resp, err := stream.Recv()
			if err != nil {
				log.Println(err)
				break
			}

			if resp.Ping != nil {
				n.lastPong = time.Now()
				continue
			}

			err = n.handler(resp)
			if err != nil {
				log.Println(err)
				continue
			}

		}

	}
}

func (n *network) send(msg *protocol.Event) {
	for retries := 5; retries > 0; retries-- {
		if n.stream != nil {
			err := n.stream.Send(msg)
			if err != nil {
				log.Printf("Error while sending msg: %v", err)
			} else {
				break
			}
		}

		time.Sleep(time.Millisecond * 200)
	}
}

func (n *network) healthPings() {
	for range time.Tick(time.Millisecond * 500) {
		n.send(&protocol.Event{
			Ping: &protocol.PingEvent{
				Nonce: uint64(rand.Int63()),
			},
		})
		go func() {
			time.Sleep(time.Second)
			if time.Since(n.lastPong) > time.Second {
				n.stahp()
				if n.stream != nil {
					n.stream.Send(&protocol.Event{
						Reason: fmt.Sprintf("client %v did not recieve pongs for a second"),
						Ready: &protocol.ReadyEvent{
							ClientReady: false,
						},
					})
				}
			}

		}()
	}
}

func (n *network) setReady(ready bool, reason string) {
	log.Printf("sending ready %v, reason=%v", ready, reason)
	n.send(&protocol.Event{
		Reason: reason,
		Ready: &protocol.ReadyEvent{
			ClientReady: ready,
		},
	})
}

func (n *network) seek(time float64, reason string) {
	log.Printf("sending seek %v, reason=%v", time, reason)
	go n.send(&protocol.Event{
		Reason: reason,
		Time: &protocol.TimeChangeEvent{
			Where: time,
		},
	})
}
