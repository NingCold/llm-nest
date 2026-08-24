import { Component, type ReactNode } from "react"

interface Props { children: ReactNode; fallback?: ReactNode }
interface State { hasError: boolean; error?: Error }

class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error }
  }
  render() {
    if (this.state.hasError) {
      return this.props.fallback ?? (
        <div style={{ padding: 20, color: "red", fontSize: 14 }}>
          <p style={{ fontWeight: "bold" }}>Error: {this.state.error?.message}</p>
          <pre style={{ marginTop: 8, fontSize: 11, whiteSpace: "pre-wrap" }}>
            {this.state.error?.stack}
          </pre>
        </div>
      )
    }
    return this.props.children
  }
}

export default ErrorBoundary