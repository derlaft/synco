package main

import (
	"bufio"
	"fmt"
	"io"
	"log"
	"os/exec"
	"sync"
)

const MPLAYER = "mpv"

type mpv struct {
	sendLock sync.Mutex
	out      io.Writer
	in       io.Reader
	cmd      *exec.Cmd
}

func start(args []string) (*mpv, error) {
	m := mpv{}

	m.cmd = exec.Command(MPLAYER, args...)

	mpvStdin, err := m.cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	m.out = mpvStdin

	mpvStdout, err := m.cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	m.in = mpvStdout

	err = m.cmd.Start()
	return &m, err
}

func (m *mpv) stop() error {
	return m.cmd.Process.Kill()
}

func (m *mpv) wait() error {
	return m.cmd.Wait()
}

func (m *mpv) execCmd(cmd string) {
	log.Printf("executing mpv command: %v", cmd)
	m.sendLock.Lock()
	_, err := fmt.Fprintf(m.out, "%s\n", cmd)
	if err != nil {
		log.Println(err)
	}
	m.sendLock.Unlock()
}

func (m *mpv) event() (string, bool) {
	scanner := bufio.NewScanner(m.in)
	if scanner.Scan() {
		return scanner.Text(), false
	}
	return "", true
}
