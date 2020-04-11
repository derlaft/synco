package main

var initSeq = []string{
	// we expect mpv to start paused, pause on more type just in case
	pauseCommand,
	// send event every time F1 is pressed
	bindF1,
	// send event every time play position is changed
	observePos,
}

const (
	bindF1           = `{"command": ["keybind", "F1", "script_message ready_pressed"]}`
	observePos       = `{"command": ["observe_property", 1, "time-pos"]}\n`
	pauseCommand     = `{"command":["set_property","pause", true]}`
	unpauseCommand   = `{"command":["set_property","pause", false]}`
	othersNotReady   = `{"command":["osd-msg", "show-text", "not ready"]}`
	waitingForOthers = `{"command":["osd-msg", "show-text", "ready"]}`
	blockingOthers   = `{"command":["osd-msg", "show-text", "not ready"]}`
	seekCommand      = `{"command": ["set_property", "time-pos", %f]}`
)
