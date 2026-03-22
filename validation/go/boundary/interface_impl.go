// Expected: exit 0
// Expected: contains=types:
// Expected: contains=Reader
// Expected: contains=FileReader

package main

import "io"

// Reader reads data from a source.
type Reader interface {
	Read(p []byte) (n int, err error)
}

// FileReader implements Reader for files.
type FileReader struct {
	path string
	r    io.Reader
}

// Read reads from the file.
func (f *FileReader) Read(p []byte) (int, error) {
	return f.r.Read(p)
}

// NewFileReader creates a new FileReader.
func NewFileReader(path string) *FileReader {
	return &FileReader{path: path}
}
