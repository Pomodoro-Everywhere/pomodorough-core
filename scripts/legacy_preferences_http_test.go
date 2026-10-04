package server

import (
	"encoding/json"
	"net/http"
	"os"
	"testing"
	"time"
)

type corePWA11Scenario struct {
	Name       string
	Operations []map[string]any
	Expected   int64
	AutoStart  []map[string]any
	Selection  []map[string]any
}

func TestCorePWA11LegacyPreferencesHTTP(t *testing.T) {
	input, err := os.ReadFile(os.Getenv("PWA11_HTTP_INPUT"))
	if err != nil {
		t.Fatal(err)
	}
	var operations struct {
		Current         map[string]any   `json:"current"`
		Native          []map[string]any `json:"native"`
		NativeAutoStart []map[string]any `json:"nativeAutoStart"`
		NativeSelection []map[string]any `json:"nativeSelection"`
	}
	if err := json.Unmarshal(input, &operations); err != nil {
		t.Fatal(err)
	}
	results := []map[string]any{}
	for _, scenario := range []corePWA11Scenario{
		{"current-public-red", []map[string]any{operations.Current}, 1800000, []map[string]any{}, []map[string]any{}},
		{"native-legacy-green", operations.Native, 2100000, operations.NativeAutoStart, operations.NativeSelection},
	} {
		now := time.UnixMilli(int64(operations.Current["hlcWallMs"].(float64))).UTC()
		results = append(results, corePWA11HTTP(t, scenario, now))
	}
	encoded, err := json.MarshalIndent(results, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(os.Getenv("PWA11_HTTP_OUTPUT"), encoded, 0600); err != nil {
		t.Fatal(err)
	}
}

func corePWA11HTTP(t *testing.T, scenario corePWA11Scenario, now time.Time) map[string]any {
	t.Helper()
	f := newServerFixture(t)
	remote := validSyncRequestJSON(now.Add(-time.Second))
	remote.Commands = []syncCommandJSON{}
	remote.DurationOperations = []syncDurationOperationJSON{validDurationOperationJSON(now.Add(-time.Second), "focus", 2100000)}
	remote.AutoStartOperations = []syncAutoStartOperationJSON{validAutoStartOperationJSON(now.Add(-time.Second), true)}
	first := postAuthenticatedJSON(t, f, "/api/v1/sync", remote)
	if first.Code != http.StatusOK {
		t.Fatalf("Remote: %d %s", first.Code, first.Body.String())
	}
	body := map[string]any{"requestId": "core-pwa11-" + scenario.Name, "deviceId": f.deviceID,
		"expectedRevision": 1, "strategy": "merge", "commands": []any{}, "taskOperations": []any{},
		"durationOperations": scenario.Operations, "autoStartOperations": scenario.AutoStart, "selectedTaskOperations": scenario.Selection}
	response := postAuthenticatedJSON(t, f, "/api/v1/bootstrap/resolve", body)
	if response.Code != http.StatusOK {
		t.Fatalf("Merge: %d %s", response.Code, response.Body.String())
	}
	var decoded map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &decoded); err != nil {
		t.Fatal(err)
	}
	if decoded["durationsMs"].(map[string]any)["focus"] != float64(scenario.Expected) {
		t.Fatalf("Wrong winning duration: %s", response.Body.String())
	}
	acknowledgement := decoded["durationAcknowledgements"].([]any)[0].(map[string]any)
	winner := scenario.Operations[0]["id"]
	if scenario.Name == "native-legacy-green" {
		winner = remote.DurationOperations[0].ID
		if acknowledgement["outcome"] != "ignored" || acknowledgement["reason"] != "superseded by newer duration operation" {
			t.Fatalf("Legacy should lose: %v", acknowledgement)
		}
		corePWA11FlagResults(t, decoded)
	}
	winningRow := corePWA11WinningRow(t, f)
	if winningRow["id"] != winner || winningRow["durationMs"] != scenario.Expected {
		t.Fatalf("Wrong SQLite winner: %v", winningRow)
	}
	return map[string]any{"case": scenario.Name, "remoteRequest": remote, "remoteResponseRaw": first.Body.String(),
		"mergeRequest": body, "responseRaw": response.Body.String(), "response": decoded, "winningSQLiteRow": winningRow}
}

func corePWA11FlagResults(t *testing.T, decoded map[string]any) {
	t.Helper()
	if decoded["autoStartBreaks"] != true || decoded["selectedTaskId"] != nil {
		t.Fatalf("Wrong flag result: %v", decoded)
	}
	auto := decoded["autoStartAcknowledgements"].([]any)
	selection := decoded["selectedTaskAcknowledgements"].([]any)
	if len(auto) != 1 || auto[0].(map[string]any)["outcome"] != "ignored" {
		t.Fatalf("Explicit false must lose to newer true: %v", auto)
	}
	if len(selection) != 1 || selection[0].(map[string]any)["outcome"] != "applied" {
		t.Fatalf("Explicit null must reach server: %v", selection)
	}
}

func corePWA11WinningRow(t *testing.T, f serverFixture) map[string]any {
	t.Helper()
	db, err := f.userStore.OpenUser(t.Context(), f.userID)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var id, deviceID, occurredAt string
	var duration, wall, counter int64
	err = db.QueryRowContext(t.Context(), `SELECT id, device_id, duration_ms, occurred_at, hlc_wall_ms, hlc_counter
		FROM duration_operations WHERE phase = 'focus' ORDER BY hlc_wall_ms DESC, hlc_counter DESC, device_id DESC, id DESC LIMIT 1`).
		Scan(&id, &deviceID, &duration, &occurredAt, &wall, &counter)
	if err != nil {
		t.Fatal(err)
	}
	return map[string]any{"id": id, "deviceId": deviceID, "durationMs": duration, "occurredAt": occurredAt, "hlcWallMs": wall, "hlcCounter": counter}
}
