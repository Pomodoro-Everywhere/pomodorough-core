package server

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"
)

func TestCorePWA12NaturalHTTP(t *testing.T) {
	f := newServerFixture(t)
	now := time.Now().UTC().Truncate(time.Millisecond)
	start := validSyncRequestJSON(now.Add(-60001 * time.Millisecond))
	start.Commands[0].PlannedDurationMs = int64Pointer(60000)
	response := postAuthenticatedJSON(t, f, "/api/v1/sync", start)
	if response.Code != http.StatusOK {
		t.Fatalf("Start HTTP status: %d %s", response.Code, response.Body.String())
	}
	var natural map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &natural); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(f.handler)
	defer server.Close()
	payload, err := json.Marshal(map[string]any{
		"url": server.URL, "userID": f.userID, "deviceID": f.deviceID,
		"accessToken": f.accessToken, "csrfToken": f.csrfToken, "nowMs": time.Now().UnixMilli(),
		"startRequest": start, "naturalResponseRaw": response.Body.String(), "naturalResponse": natural,
	})
	if err != nil {
		t.Fatal(err)
	}
	output := os.Getenv("CORE_PWA12_HTTP_FIXTURE")
	if err := os.WriteFile(output, payload, 0600); err != nil {
		t.Fatal(err)
	}
	waitCorePWA12HTTP(t, output)
}

func waitCorePWA12HTTP(t *testing.T, output string) {
	deadline := time.NewTimer(90 * time.Second)
	defer deadline.Stop()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-t.Context().Done():
			t.Fatal(t.Context().Err())
		case <-deadline.C:
			t.Fatal("PWA12 HTTP fixture was not released")
		case <-ticker.C:
			if _, err := os.Stat(output + ".stop"); err == nil {
				return
			}
		}
	}
}
