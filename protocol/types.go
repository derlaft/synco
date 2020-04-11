package protocol

// MessageType allows to understand what this message is
type MessageType string

// Various possible message types sent by server
const (
	// Hello message contains UserID and only sent once
	HelloMessage = "h"
	// ServerReady message is sent by server to all clients
	// it means clients should immediately start playback
	ServerReadyMessage = "r"
	// ServerUnreadyMessage is sent by server to all clients
	// it means clients should immediately stop playback
	ServerUnreadyMessage = "!r"
	// ServerPositionMessage means that one of the clients scrolled the video
	// it means all the clients should change position to given one
	ServerSeekMessage = ">>"
)

// Various possible message types sent by client
const (
	// ClientReadyMessage is sent by client when it's ready
	ClientReadyMessage = "r"
	// ClientUnreadyMessage is sent by client when it's not ready anymore
	ClientUnreadyMessage = "!r"
	// ClientPosition message is sent by client all the time
	// it works like a ping and health check
	ClientPositionMessage = "??"
	// ClientSeekMessage indicates that clients want to force-change position
	ClientSeekMessage = ">>"
)

// Message that is being sent over network
type Message struct {
	ID       MessageType `json:"t"`
	UserID   string      `json:"u,omitempty"`
	Position float64     `json:"p,omitempty"`
}
