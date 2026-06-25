import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles/tokens.css";

function App() {
  const [greeting, setGreeting] = useState("");
  const [backendEvent, setBackendEvent] = useState("");
  const [name, setName] = useState("");

  useEffect(() => {
    // Listen for backend-to-frontend events (verifies back-to-front IPC)
    const unlisten = listen<string>("backend-ready", (event) => {
      setBackendEvent(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function handleGreet() {
    // Verifies front-to-back IPC
    const result = await invoke<string>("greet", { name: name || "World" });
    setGreeting(result);
  }

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      {/* Sidebar placeholder */}
      <aside
        style={{
          width: 248,
          background: "var(--bg)",
          borderRight: "1px solid var(--border)",
          padding: "var(--space-4)",
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-3)",
        }}
      >
        <h2
          style={{
            fontFamily: "var(--font-display)",
            fontSize: "var(--text-lg)",
            letterSpacing: "var(--tracking-display)",
          }}
        >
          Cognest
        </h2>
        <p style={{ color: "var(--muted)", fontSize: "var(--text-sm)" }}>
          Sidebar — 导航区域
        </p>
      </aside>

      {/* Main content */}
      <main
        style={{
          flex: 1,
          padding: "var(--space-8)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: "var(--space-4)",
        }}
      >
        <h1
          style={{
            fontFamily: "var(--font-display)",
            fontSize: "var(--text-2xl)",
            letterSpacing: "var(--tracking-display)",
            lineHeight: "var(--leading-tight)",
          }}
        >
          Cognest
        </h1>
        <p style={{ color: "var(--muted)" }}>IPC 双向通信验证</p>

        <div style={{ display: "flex", gap: "var(--space-2)" }}>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="输入名字..."
            style={{
              padding: "var(--space-2) var(--space-3)",
              borderRadius: "var(--radius-sm)",
              border: "1px solid var(--border)",
              fontFamily: "var(--font-body)",
              fontSize: "var(--text-sm)",
            }}
          />
          <button
            onClick={handleGreet}
            style={{
              padding: "var(--space-2) var(--space-4)",
              borderRadius: "var(--radius-sm)",
              background: "var(--accent)",
              color: "var(--accent-on)",
              fontSize: "var(--text-sm)",
              fontWeight: 500,
            }}
          >
            Greet
          </button>
        </div>

        {greeting && (
          <p
            style={{
              padding: "var(--space-3)",
              background: "var(--bg)",
              borderRadius: "var(--radius-md)",
              boxShadow: "var(--elev-ring)",
            }}
          >
            ✓ Front→Back: {greeting}
          </p>
        )}
        {backendEvent && (
          <p
            style={{
              padding: "var(--space-3)",
              background: "var(--bg)",
              borderRadius: "var(--radius-md)",
              boxShadow: "var(--elev-ring)",
            }}
          >
            ✓ Back→Front: {backendEvent}
          </p>
        )}
      </main>
    </div>
  );
}

export default App;
