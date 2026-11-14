package main

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"time"
)

func main() {
	n, err := strconv.ParseUint(os.Args[1], 10, 64)
	if err != nil {
		panic(err)
	}
	size, err := strconv.Atoi(os.Args[2])
	if err != nil || size < 8 {
		panic("invalid size")
	}
	buf := make([]byte, size)
	for j := range buf {
		buf[j] = byte(j)
	}
	var digest [32]byte
	start := time.Now()
	for i := uint64(0); i < n; i++ {
		binary.LittleEndian.PutUint64(buf[size-8:], i)
		digest = sha256.Sum256(buf)
	}
	elapsed := time.Since(start).Seconds()
	result, err := json.Marshal(map[string]any{
		"seconds": elapsed, "hashes_per_second": float64(n) / elapsed,
		"digest": hex.EncodeToString(digest[:]),
	})
	if err != nil {
		panic(err)
	}
	fmt.Println(string(result))
}
