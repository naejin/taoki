// Expected: exit 0
// Expected: sections=imports,types,fns
// Expected: contains=Server
// Expected: contains=Start
// Expected: contains=handleRequest

package main

import (
	"fmt"
	"net/http"
)

// Server handles HTTP requests.
type Server struct {
	addr string
	mux  *http.ServeMux
}

// NewServer creates a new server.
func NewServer(addr string) *Server {
	return &Server{addr: addr, mux: http.NewServeMux()}
}

// Start starts the server.
func (s *Server) Start() error {
	return http.ListenAndServe(s.addr, s.mux)
}

func handleRequest(w http.ResponseWriter, r *http.Request) {
	fmt.Fprintf(w, "Hello, %s!", r.URL.Path)
}
