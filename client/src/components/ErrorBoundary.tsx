import React, { ErrorInfo, ReactNode } from 'react';
import { AlertTriangle, RefreshCw, Home, FileText } from 'lucide-react';
import { Button } from '../features/design-system/components';
import { createLogger } from '../utils/loggerConfig';
import { unifiedApiClient } from '../services/api/UnifiedApiClient';

const logger = createLogger('ErrorBoundary');

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
  errorCount: number;
}

interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: (error: Error, errorInfo: ErrorInfo, resetError: () => void) => ReactNode;
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
  resetKeys?: Array<string | number>;
}

class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  private resetTimeoutId: number | null = null;
  private previousResetKeys: Array<string | number> = [];

  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = {
      hasError: false,
      error: null,
      errorInfo: null,
      errorCount: 0
    };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    
    return {
      hasError: true,
      error,
      errorInfo: null,
      errorCount: 0
    };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    
    logger.error('React Error Boundary caught an error:', {
      error: error.toString(),
      stack: error.stack,
      componentStack: errorInfo.componentStack
    });

    
    if (this.props.onError) {
      this.props.onError(error, errorInfo);
    }

    
    this.logErrorToServer(error, errorInfo);

    
    this.setState(prevState => ({
      error,
      errorInfo,
      errorCount: prevState.errorCount + 1
    }));
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps) {
    const { resetKeys } = this.props;
    const hasResetKeysChanged = resetKeys !== prevProps.resetKeys &&
      JSON.stringify(resetKeys) !== JSON.stringify(this.previousResetKeys);

    if (hasResetKeysChanged) {
      this.previousResetKeys = resetKeys || [];
      this.resetError();
    }
  }

  private async logErrorToServer(error: Error, errorInfo: ErrorInfo) {
    try {
      const errorData = {
        message: error.message,
        stack: error.stack,
        componentStack: errorInfo.componentStack,
        timestamp: new Date().toISOString(),
        userAgent: navigator.userAgent,
        url: window.location.href
      };

      await unifiedApiClient.post('/errors/log', errorData);
    } catch (logError) {
      
      logger.warn('Failed to log error to server:', logError);
    }
  }

  resetError = () => {
    this.setState({
      hasError: false,
      error: null,
      errorInfo: null,
      errorCount: 0
    });
  };

  handleReload = () => {
    window.location.reload();
  };

  handleGoHome = () => {
    window.location.href = '/';
  };

  render() {
    const { hasError, error, errorInfo } = this.state;

    if (hasError && error && errorInfo) {
      
      if (this.props.fallback) {
        return this.props.fallback(error, errorInfo, this.resetError);
      }

      
      return (
        <div role="alert" aria-live="assertive" className="min-h-screen bg-background flex items-center justify-center p-4">
          <div className="max-w-2xl w-full">
            <div className="bg-card rounded-lg shadow-lg p-8 space-y-6">
              {}
              <div className="flex items-center space-x-4">
                <div className="bg-destructive/10 p-3 rounded-full">
                  <AlertTriangle className="h-8 w-8 text-destructive" />
                </div>
                <div>
                  <h1 className="text-2xl font-bold text-foreground">
                    Something went wrong
                  </h1>
                  <p className="text-sm text-muted-foreground mt-1">
                    An unexpected error occurred in the application
                  </p>
                </div>
              </div>

              {}
              <div className="space-y-4">
                <div className="bg-muted/50 rounded-md p-4">
                  <h3 className="font-semibold text-sm mb-2">Error Message</h3>
                  <p className="text-sm font-mono text-destructive">
                    {error.message}
                  </p>
                </div>

                {}
                {process.env.NODE_ENV === 'development' && (
                  <details className="bg-muted/50 rounded-md p-4">
                    <summary className="cursor-pointer font-semibold text-sm">
                      Technical Details (Development Only)
                    </summary>
                    <div className="mt-4 space-y-4">
                      <div>
                        <h4 className="text-xs font-semibold mb-1">Stack Trace</h4>
                        <pre className="text-xs overflow-auto max-h-40 p-2 bg-background rounded">
                          {error.stack}
                        </pre>
                      </div>
                      <div>
                        <h4 className="text-xs font-semibold mb-1">Component Stack</h4>
                        <pre className="text-xs overflow-auto max-h-40 p-2 bg-background rounded">
                          {errorInfo.componentStack}
                        </pre>
                      </div>
                    </div>
                  </details>
                )}
              </div>

              {}
              <div className="flex flex-wrap gap-3">
                <Button
                  onClick={this.resetError}
                  className="flex-1 sm:flex-none"
                >
                  <RefreshCw className="h-4 w-4 mr-2" />
                  Try Again
                </Button>
                <Button
                  onClick={this.handleReload}
                  variant="outline"
                  className="flex-1 sm:flex-none"
                >
                  <RefreshCw className="h-4 w-4 mr-2" />
                  Reload Page
                </Button>
                <Button
                  onClick={this.handleGoHome}
                  variant="outline"
                  className="flex-1 sm:flex-none"
                >
                  <Home className="h-4 w-4 mr-2" />
                  Go Home
                </Button>
              </div>

              {}
              <div className="text-sm text-muted-foreground space-y-2 pt-4 border-t">
                <p>
                  This error has been automatically reported. If the problem persists:
                </p>
                <ul className="list-disc list-inside space-y-1 ml-4">
                  <li>Try refreshing the page</li>
                  <li>Clear your browser cache</li>
                  <li>Check your internet connection</li>
                  <li>Contact support if the issue continues</li>
                </ul>
              </div>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

export default ErrorBoundary;

// Convenience hook to trigger error boundary
export function useErrorBoundary() {
  const [, setState] = React.useState({});
  
  const resetError = React.useCallback(() => {
    setState({});
  }, []);

  const captureError = React.useCallback((error: Error) => {
    throw error;
  }, []);

  return { resetError, captureError };
}