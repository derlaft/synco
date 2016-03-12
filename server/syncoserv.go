package main

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"strings"
	"sync"
	"time"
)

func split(cl *client, in io.Reader) {
	scanner := bufio.NewScanner(in)
	for scanner.Scan() {
		msg := scanner.Text()
		fmt.Println("got msg", msg)
		tokens := strings.Split(msg, " ")
		if len(tokens) == 2 {
			switch tokens[0] {
			case "seek":
				go broadcast(in, msg)
			case "ready":
				cl.ready = tokens[1] == "true"
				on_ready_changed()
			}
		}
	}
}

func on_disconnect() {
	for _, cl := range clients {
		cl.Lock()
		cl.ready = false
		cl.Unlock()
	}
	on_ready_changed()
}

func on_ready_changed() {
	seto := len(clients) > 1
	for _, some_client := range clients {
		seto = seto && some_client.ready
	}

	go broadcast(nil, fmt.Sprintf("seto %t", seto))
}

func ping() {
	for {
		select {
		case <-time.After(time.Second):
			go broadcast(nil, "ping")
		}
	}
}

func broadcast(src io.Reader, msg string) {
	fmt.Fprintf(os.Stderr, "Broadcasting message %v\n", msg)
	for c, _ := range clients {
		if c != src {
			err := sendto(c, msg)
			if err != nil {
				fmt.Fprintf(os.Stderr, "Client socket err: %v\n", err)
				delete(clients, c)
				c.Close()
				on_disconnect()
			}
		}
	}
}

func sendto(client net.Conn, msg string) error {
	lock := clients[client]
	lock.Lock()
	defer lock.Unlock()

	writer := bufio.NewWriter(client)
	_, err := writer.WriteString(msg + "\n")
	if err != nil {
		return err
	}
	err = writer.Flush()
	if err != nil {
		return err
	}
	return nil
}

var clients clientsMap

type clientsMap map[net.Conn]*client

type client struct {
	ready bool
	sync.Mutex
}

func main() {

	ln, err := net.Listen("tcp", "0.0.0.0:4042")
	if err != nil {
		fmt.Fprintf(os.Stderr, "Could not bind socket: %v", err)
		return
	}

	clients = make(clientsMap)
	go ping()

	for {
		conn, err := ln.Accept()
		fmt.Fprintf(os.Stderr, "Accepting client %+v\n", conn)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error %v while accepting socket", err)
			continue
		}

		cl := &client{}
		clients[conn] = cl
		on_ready_changed()
		go split(cl, conn)
	}

}
