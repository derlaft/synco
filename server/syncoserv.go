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
			case "pong":
				cl.lastMsg = time.Now()

			}

		}
	}
}

func on_disconnect() {
	clients.RLock()
	for _, cl := range clients.Map {
		cl.ready = false
	}
	clients.RUnlock()

	go on_ready_changed()
}

func on_ready_changed() {
	clients.RLock()

	seto := len(clients.Map) > 1

	fmt.Println("TESTO", clients.Map)

	for _, some_client := range clients.Map {
		fmt.Println("TESTO -- ", some_client)
		seto = seto && some_client.ready
	}
	clients.RUnlock()

	fmt.Println("TESTO", seto)
	broadcast(nil, fmt.Sprintf("seto %t", seto))
}

func ping() {
	for {
		select {
		case <-time.After(time.Second):
			go broadcast(nil, "ping")
			go checkPings()
		}
	}
}

func checkPings() {
	clients.RLock()

	for conn, cl := range clients.Map {
		if time.Since(cl.lastMsg) > time.Second*2 {
			go disconnect(conn)
		}
	}

	clients.RUnlock()
}

func disconnect(conn net.Conn) {
	fmt.Println("DISCON!")

	clients.Lock()
	delete(clients.Map, conn)
	clients.Unlock()

	conn.Close()

	go on_disconnect()
}

func broadcast(src io.Reader, msg string) {
	fmt.Fprintf(os.Stderr, "Broadcasting message %v\n", msg)

	fmt.Println("BR >>")
	clients.RLock()
	fmt.Println("BR <<")

	for c, _ := range clients.Map {
		if c != src {
			err := sendto(c, msg)
			if err != nil {
				fmt.Fprintf(os.Stderr, "Client socket err: %v\n", err)
				go disconnect(c)
			}
		}
	}

	clients.RUnlock()
}

func sendto(client net.Conn, msg string) error {

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

var (
	clients *clientsMap = &clientsMap{
		Map: make(map[net.Conn]*client),
	}
)

type clientsMap struct {
	Map map[net.Conn]*client
	sync.RWMutex
}

type client struct {
	ready   bool
	lastMsg time.Time
}

func main() {

	ln, err := net.Listen("tcp", "0.0.0.0:4042")
	if err != nil {
		fmt.Fprintf(os.Stderr, "Could not bind socket: %v", err)
		return
	}

	go ping()

	for {
		conn, err := ln.Accept()
		fmt.Fprintf(os.Stderr, "Accepting client %+v\n", conn)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error %v while accepting socket", err)
			continue
		}

		cl := &client{
			lastMsg: time.Now(),
		}

		clients.Lock()
		clients.Map[conn] = cl
		clients.Unlock()

		on_ready_changed()
		go split(cl, conn)
	}

}
