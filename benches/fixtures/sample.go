package service

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"sync"
	"time"
)

const (
	MaxRetries       = 3
	DefaultTimeoutMs = 5000
)

// ServiceError represents a domain-layer error.
type ServiceError struct {
	Message string
	Code    string
	Status  int
}

func (e *ServiceError) Error() string {
	return fmt.Sprintf("[%s] %s", e.Code, e.Message)
}

// Indexable is the interface for items that can be indexed.
type Indexable interface {
	ID() string
	Kind() string
	Score() float64
}

// ClientConfig holds configuration for the HTTP client.
type ClientConfig struct {
	BaseURL         string
	TimeoutMs       int
	MaxRetries      int
	Headers         map[string]string
	UserAgent       string
	FollowRedirects bool
	VerifySSL       bool
	Proxy           string
}

// DefaultClientConfig returns a ClientConfig with sensible defaults.
func DefaultClientConfig(baseURL string) ClientConfig {
	return ClientConfig{
		BaseURL:         baseURL,
		TimeoutMs:       DefaultTimeoutMs,
		MaxRetries:      MaxRetries,
		Headers:         make(map[string]string),
		UserAgent:       "taoki/0.1",
		FollowRedirects: true,
		VerifySSL:       true,
	}
}

// Role represents a user's permission level.
type Role int

const (
	RoleViewer Role = iota
	RoleEditor
	RoleAdmin
)

// User is a domain entity representing an application user.
type User struct {
	ID    string
	Name  string
	Email string
	Role  Role
}

func (u *User) GetID() string    { return u.ID }
func (u *User) Kind() string     { return "user" }
func (u *User) Score() float64   { return 1.0 }

// ClientService provides HTTP-backed service operations.
type ClientService struct {
	mu     sync.Mutex
	config ClientConfig
	client *http.Client
}

// NewClientService creates a new ClientService.
func NewClientService(config ClientConfig) *ClientService {
	return &ClientService{
		config: config,
		client: &http.Client{
			Timeout: time.Duration(config.TimeoutMs) * time.Millisecond,
		},
	}
}

// FetchUser retrieves a user by ID.
func (s *ClientService) FetchUser(ctx context.Context, id string) (*User, error) {
	if id == "" {
		return nil, &ServiceError{Message: "id cannot be empty", Code: "INVALID_INPUT", Status: 400}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.config.BaseURL+"/users/"+id, nil)
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}
	resp, err := s.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("do request: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNotFound {
		return nil, &ServiceError{Message: "user not found", Code: "NOT_FOUND", Status: 404}
	}
	return &User{ID: id, Name: "Alice", Email: "alice@example.com"}, nil
}

// PaginatedResult holds a page of results.
type PaginatedResult[T any] struct {
	Items   []T
	Total   int
	Page    int
	PerPage int
}

// Paginate returns a page of items from a slice.
func Paginate[T any](items []T, page, perPage int) PaginatedResult[T] {
	total := len(items)
	start := (page - 1) * perPage
	if start > total {
		start = total
	}
	end := start + perPage
	if end > total {
		end = total
	}
	return PaginatedResult[T]{Items: items[start:end], Total: total, Page: page, PerPage: perPage}
}

// ParseHeader splits a raw "Key: Value" header string.
func ParseHeader(raw string) (string, string, error) {
	for i, c := range raw {
		if c == ':' {
			return trim(raw[:i]), trim(raw[i+1:]), nil
		}
	}
	return "", "", errors.New("invalid header: missing colon")
}

func trim(s string) string {
	start, end := 0, len(s)
	for start < end && (s[start] == ' ' || s[start] == '\t') {
		start++
	}
	for end > start && (s[end-1] == ' ' || s[end-1] == '\t') {
		end--
	}
	return s[start:end]
}

func internalHash(data []byte) uint64 {
	var h uint64 = 0xcbf29ce484222325
	for _, b := range data {
		h ^= uint64(b)
		h *= 0x100000001b3
	}
	return h
}
