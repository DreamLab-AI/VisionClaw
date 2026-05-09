import React, { ErrorInfo, ReactNode } from 'react';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('FeatureErrorBoundary');

interface Props {
  feature: string;
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

class FeatureErrorBoundary extends React.Component<Props, State> {
  state: State = { hasError: false, error: null };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    logger.error(`[${this.props.feature}] crashed:`, {
      error: error.message,
      componentStack: errorInfo.componentStack,
    });
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) return this.props.fallback;
      return (
        <div
          role="alert"
          style={{
            padding: '1rem',
            margin: '0.5rem',
            background: 'rgba(220, 38, 38, 0.1)',
            border: '1px solid rgba(220, 38, 38, 0.3)',
            borderRadius: '0.5rem',
            color: '#fca5a5',
            fontSize: '0.875rem',
          }}
        >
          <strong>{this.props.feature}</strong> encountered an error.{' '}
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            style={{
              textDecoration: 'underline',
              cursor: 'pointer',
              background: 'none',
              border: 'none',
              color: 'inherit',
              font: 'inherit',
            }}
          >
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export default FeatureErrorBoundary;
