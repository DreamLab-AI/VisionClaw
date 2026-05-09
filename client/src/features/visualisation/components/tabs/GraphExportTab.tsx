

import React, { useState, useCallback } from 'react';
import {
  Download,
  Share,
  // @ts-ignore - Icon exists in lucide-react but types may be outdated
  FileImage,
  FileText,
  Database,
  Code,
  Globe,
  Mail,
  Copy,
  ExternalLink,
  AlertCircle,
  CheckCircle,
  Upload,
  Loader2
} from 'lucide-react';
import { Button } from '@/features/design-system/components/Button';
import { Switch } from '@/features/design-system/components/Switch';
import { Label } from '@/features/design-system/components/Label';
import { Badge } from '@/features/design-system/components/Badge';
import { 
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/features/design-system/components/Select';
import { Card, CardContent, CardHeader, CardTitle } from '@/features/design-system/components/Card';
import { Input } from '@/features/design-system/components/Input';
import { Textarea } from '@/features/design-system/components/Textarea';
import { Separator } from '@/features/design-system/components/Separator';
import { toast } from '@/features/design-system/components/Toast';
import {
  exportGraph,
  shareGraph,
  publishGraph,
  generateEmbedCode,
  generateApiEndpoint,
  copyToClipboard,
  ExportOptions,
  ShareOptions,
  PublishMetadata
} from '@/api/exportApi';
import ShareSettingsDialog from '../dialogs/ShareSettingsDialog';
import ExportFormatDialog from '../dialogs/ExportFormatDialog';
import ShareLinkManager from '../dialogs/ShareLinkManager';
import PublishGraphDialog from '../dialogs/PublishGraphDialog';

interface GraphExportTabProps {
  graphId?: string;
  graphData?: any;
  onExport?: (format: string, options: any) => void;
}

export const GraphExportTab: React.FC<GraphExportTabProps> = ({ 
  graphId = 'default',
  graphData,
  onExport
}) => {
  
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [showShareDialog, setShowShareDialog] = useState(false);
  const [showShareManager, setShowShareManager] = useState(false);
  const [showPublishDialog, setShowPublishDialog] = useState(false);
  const [isExporting, setIsExporting] = useState(false);

  
  const [shareUrl, setShareUrl] = useState('');
  const [isSharing, setIsSharing] = useState(false);
  const [isPublishing, setIsPublishing] = useState(false);

  
  const [embedCode, setEmbedCode] = useState('');
  const [apiEndpoint, setApiEndpoint] = useState('');
  const [shareId, setShareId] = useState('');

  
  const handleExport = useCallback(async (options: ExportOptions) => {
    if (!graphData) {
      toast({
        title: "No Data",
        description: "No graph data available to export",
        variant: "destructive"
      });
      return;
    }

    setIsExporting(true);
    setShowExportDialog(false);

    try {
      toast({
        title: "Starting Export",
        description: `Preparing ${options.format.toUpperCase()} export...`
      });

      const result = await exportGraph(graphData, options);

      if (result.success) {
        toast({
          title: "Export Complete",
          description: `Graph exported as ${options.format.toUpperCase()} successfully`,
          // Note: toast action would go here if needed
        });
        onExport?.(options.format, options);
      } else {
        throw new Error(result.message || 'Export failed');
      }
    } catch (error) {
      toast({
        title: "Export Failed",
        description: error instanceof Error ? error.message : "An error occurred during export",
        variant: "destructive"
      });
    } finally {
      setIsExporting(false);
    }
  }, [graphData, onExport]);

  
  const handleShare = useCallback(async (options: ShareOptions) => {
    if (!graphData) {
      toast({
        title: "No Data",
        description: "No graph data available to share",
        variant: "destructive"
      });
      return;
    }

    setIsSharing(true);
    setShowShareDialog(false);

    try {
      toast({
        title: "Creating Share Link",
        description: "Generating secure sharing URL..."
      });

      const result = await shareGraph(graphData, options);

      if (result.success && result.shareUrl) {
        setShareUrl(result.shareUrl);
        setShareId(result.shareId || '');

        
        const copied = await copyToClipboard(result.shareUrl);

        toast({
          title: "Share Link Created",
          description: copied ? "Link copied to clipboard automatically" : "Share link created successfully"
        });
      } else {
        throw new Error(result.message || 'Share failed');
      }
    } catch (error) {
      toast({
        title: "Share Link Failed",
        description: error instanceof Error ? error.message : "Could not create share link",
        variant: "destructive"
      });
    } finally {
      setIsSharing(false);
    }
  }, [graphData]);

  
  const handlePublish = useCallback(async (metadata: PublishMetadata) => {
    if (!graphData) {
      toast({
        title: "No Data",
        description: "No graph data available to publish",
        variant: "destructive"
      });
      return;
    }

    setIsPublishing(true);
    setShowPublishDialog(false);

    try {
      toast({
        title: "Publishing Graph",
        description: "Uploading to repository..."
      });

      const result = await publishGraph(graphData, metadata);

      if (result.success) {
        toast({
          title: "Graph Published",
          description: "Your graph is now available in the public repository",
          // Note: toast action would go here if needed
        });
      } else {
        throw new Error(result.message || 'Publish failed');
      }
    } catch (error) {
      toast({
        title: "Publish Failed",
        description: error instanceof Error ? error.message : "Could not publish graph",
        variant: "destructive"
      });
    } finally {
      setIsPublishing(false);
    }
  }, [graphData]);

  const handleGenerateEmbedCode = useCallback(async () => {
    if (!shareId && !shareUrl) {
      toast({
        title: "Create Share Link First",
        description: "You need to create a share link before generating embed code",
        variant: "destructive"
      });
      return;
    }

    const currentShareId = shareId || shareUrl.split('/').pop() || graphId;
    const embedHtml = generateEmbedCode(currentShareId, {
      width: 800,
      height: 600,
      interactive: true,
      showControls: true,
      theme: 'auto'
    });

    setEmbedCode(embedHtml);
    const success = await copyToClipboard(embedHtml);

    toast({
      title: success ? "Embed Code Generated" : "Generation Complete",
      description: success ? "HTML embed code copied to clipboard" : "Embed code generated",
      variant: success ? "default" : "destructive"
    });
  }, [shareId, shareUrl, graphId]);

  const handleGenerateApiEndpoint = useCallback(async () => {
    if (!shareId && !shareUrl) {
      toast({
        title: "Create Share Link First",
        description: "You need to create a share link before generating API endpoint",
        variant: "destructive"
      });
      return;
    }

    const currentShareId = shareId || shareUrl.split('/').pop() || graphId;
    const endpoint = generateApiEndpoint(currentShareId);

    setApiEndpoint(endpoint);
    const success = await copyToClipboard(endpoint);

    toast({
      title: success ? "API Endpoint Generated" : "Generation Complete",
      description: success ? "REST API endpoint copied to clipboard" : "API endpoint generated",
      variant: success ? "default" : "destructive"
    });
  }, [shareId, shareUrl, graphId]);

  return (
    <>
      <div className="space-y-4">
        {}
        <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Download className="h-4 w-4" />
            Data Export
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="text-sm text-muted-foreground mb-3">
            Choose from multiple export formats including JSON, GraphML, images, and more.
          </div>

          <Button
            onClick={() => setShowExportDialog(true)}
            disabled={isExporting}
            className="w-full"
          >
            {isExporting ? (
              <>
                <Loader2 className="h-3 w-3 mr-2 animate-spin" />
                Exporting...
              </>
            ) : (
              <>
                <Download className="h-3 w-3 mr-2" />
                Choose Export Format
              </>
            )}
          </Button>
        </CardContent>
      </Card>

      {}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Share className="h-4 w-4" />
            Sharing & Collaboration
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="text-sm text-muted-foreground mb-3">
            Generate secure shareable links with custom permissions and expiry settings.
          </div>

          <div className="grid grid-cols-2 gap-2">
            <Button
              onClick={() => setShowShareDialog(true)}
              disabled={isSharing}
              variant="outline"
              className="w-full"
            >
              {isSharing ? (
                <>
                  <Loader2 className="h-3 w-3 mr-2 animate-spin" />
                  Creating...
                </>
              ) : (
                <>
                  <Globe className="h-3 w-3 mr-2" />
                  New Share Link
                </>
              )}
            </Button>

            <Button
              onClick={() => setShowShareManager(true)}
              variant="outline"
              className="w-full"
            >
              <Share className="h-3 w-3 mr-2" />
              Manage Shares
            </Button>
          </div>
          
          {shareUrl && (
            <div className="p-2 bg-muted rounded space-y-2">
              <div className="flex items-center justify-between">
                <Label className="text-xs text-green-600 flex items-center gap-1">
                  <CheckCircle className="h-3 w-3" />
                  Share Link Created
                </Label>
                <Button 
                  size="sm" 
                  variant="ghost"
                  onClick={() => {
                    navigator.clipboard.writeText(shareUrl);
                    toast({ title: "Link copied to clipboard" });
                  }}
                >
                  <Copy className="h-3 w-3" />
                </Button>
              </div>
              <div className="text-xs font-mono p-1 bg-background rounded border break-all">
                {shareUrl}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Code className="h-4 w-4" />
            Web Integration
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid grid-cols-2 gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleGenerateEmbedCode}
              className="w-full"
              disabled={!shareUrl && !shareId}
            >
              <FileText className="h-3 w-3 mr-1" />
              Embed Code
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleGenerateApiEndpoint}
              className="w-full"
              disabled={!shareUrl && !shareId}
            >
              <Database className="h-3 w-3 mr-1" />
              API Endpoint
            </Button>
          </div>
          
          {embedCode && (
            <div className="space-y-1">
              <Label className="text-xs">HTML Embed Code</Label>
              <Textarea
                value={embedCode}
                readOnly
                className="text-xs font-mono"
                rows={4}
                onClick={(e) => (e.target as HTMLTextAreaElement).select()}
              />
            </div>
          )}
          
          {apiEndpoint && (
            <div className="space-y-1">
              <Label className="text-xs">REST API Endpoint</Label>
              <div className="flex items-center gap-2">
                <Input
                  value={apiEndpoint}
                  readOnly
                  className="text-xs font-mono flex-1"
                  onClick={(e) => (e.target as HTMLInputElement).select()}
                />
                <Button 
                  size="sm" 
                  variant="ghost"
                  onClick={() => window.open(apiEndpoint, '_blank', 'noopener,noreferrer')}
                >
                  <ExternalLink className="h-3 w-3" />
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Upload className="h-4 w-4" />
            Publishing
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="text-sm text-muted-foreground mb-3">
            Publish your graph to the public repository for others to discover and cite.
          </div>

          <Button
            onClick={() => setShowPublishDialog(true)}
            disabled={isPublishing}
            className="w-full"
            variant="outline"
          >
            {isPublishing ? (
              <>
                <Loader2 className="h-3 w-3 mr-2 animate-spin" />
                Publishing...
              </>
            ) : (
              <>
                <Upload className="h-3 w-3 mr-2" />
                Publish to Repository
              </>
            )}
          </Button>
        </CardContent>
      </Card>

      {}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Download className="h-4 w-4" />
            Quick Export
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <div className="grid grid-cols-2 gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleExport({ format: 'png', resolution: '1920x1080', quality: 90 })}
              className="w-full"
              disabled={isExporting}
            >
              <FileImage className="h-3 w-3 mr-1" />
              Save PNG
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleExport({ format: 'json', includeMetadata: true })}
              className="w-full"
              disabled={isExporting}
            >
              <Database className="h-3 w-3 mr-1" />
              Export JSON
            </Button>
          </div>
        </CardContent>
      </Card>
      </div>

      {}
      <ExportFormatDialog
        open={showExportDialog}
        onClose={() => setShowExportDialog(false)}
        onExport={handleExport}
        graphData={graphData}
        isLoading={isExporting}
      />

      <ShareSettingsDialog
        open={showShareDialog}
        onClose={() => setShowShareDialog(false)}
        onShare={handleShare}
        isLoading={isSharing}
      />

      <ShareLinkManager
        open={showShareManager}
        onClose={() => setShowShareManager(false)}
      />

      <PublishGraphDialog
        open={showPublishDialog}
        onClose={() => setShowPublishDialog(false)}
        onPublish={handlePublish}
        isLoading={isPublishing}
      />
    </>
  );
};

export default GraphExportTab;