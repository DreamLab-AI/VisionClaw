

import React, { useState, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/features/design-system/components/Dialog';
import { Button } from '@/features/design-system/components/Button';
import { Badge } from '@/features/design-system/components/Badge';
import { Card, CardContent } from '@/features/design-system/components/Card';
import { Separator } from '@/features/design-system/components/Separator';
import { toast } from '@/features/design-system/components/Toast';
import {
  Globe,
  Copy,
  Trash2,
  Edit3,
  Calendar,
  Shield,
  Eye,
  ExternalLink,
  Loader2
} from 'lucide-react';
import { getUserSharedGraphs, deleteSharedGraph, copyToClipboard } from '@/api/exportApi';

interface SharedGraph {
  id: string;
  title: string;
  description?: string;
  shareUrl: string;
  createdAt: string;
  expiresAt?: string;
  isPasswordProtected: boolean;
  viewCount: number;
  permissions: {
    allowDownload: boolean;
    allowComment: boolean;
    allowEdit: boolean;
  };
}

interface ShareLinkManagerProps {
  open: boolean;
  onClose: () => void;
  onEdit?: (shareId: string) => void;
}

export const ShareLinkManager: React.FC<ShareLinkManagerProps> = ({
  open,
  onClose,
  onEdit
}) => {
  const [sharedGraphs, setSharedGraphs] = useState<SharedGraph[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      loadSharedGraphs();
    }
  }, [open]);

  const loadSharedGraphs = async () => {
    setIsLoading(true);
    try {
      const graphs = await getUserSharedGraphs();
      setSharedGraphs(graphs);
    } catch (error) {
      toast({
        title: "Failed to Load",
        description: "Could not load shared graphs",
        variant: "destructive"
      });
    } finally {
      setIsLoading(false);
    }
  };

  const handleCopyLink = async (shareUrl: string, title: string) => {
    const success = await copyToClipboard(shareUrl);
    if (success) {
      toast({
        title: "Link Copied",
        description: `Share link for "${title}" copied to clipboard`
      });
    } else {
      toast({
        title: "Copy Failed",
        description: "Could not copy link to clipboard",
        variant: "destructive"
      });
    }
  };

  const handleDelete = async (shareId: string, title: string) => {
    setDeletingId(shareId);
    try {
      const success = await deleteSharedGraph(shareId);
      if (success) {
        setSharedGraphs(prev => prev.filter(graph => graph.id !== shareId));
        toast({
          title: "Share Deleted",
          description: `"${title}" is no longer shared`
        });
      } else {
        throw new Error('Delete failed');
      }
    } catch (error) {
      toast({
        title: "Delete Failed",
        description: "Could not delete shared graph",
        variant: "destructive"
      });
    } finally {
      setDeletingId(null);
    }
  };

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-GB', {
      day: 'numeric',
      month: 'short',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });
  };

  const isExpired = (expiresAt?: string) => {
    if (!expiresAt) return false;
    return new Date(expiresAt) < new Date();
  };

  const getStatusBadge = (graph: SharedGraph) => {
    if (isExpired(graph.expiresAt)) {
      return <Badge variant="destructive">Expired</Badge>;
    }
    if (graph.isPasswordProtected) {
      return <Badge variant="secondary">Protected</Badge>;
    }
    return <Badge variant="default">Active</Badge>;
  };

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[90vh]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe className="h-5 w-5" />
            Manage Shared Graphs
            {sharedGraphs.length > 0 && (
              <Badge variant="outline" className="ml-auto">
                {sharedGraphs.length} shared
              </Badge>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin" />
              <span className="ml-2">Loading shared graphs...</span>
            </div>
          ) : sharedGraphs.length === 0 ? (
            <div className="text-center py-8 text-muted-foreground">
              <Globe className="h-12 w-12 mx-auto mb-4 opacity-50" />
              <p>No shared graphs yet</p>
              <p className="text-sm">Create a share link to see it here</p>
            </div>
          ) : (
            <div className="space-y-3 max-h-96 overflow-y-auto">
              {sharedGraphs.map((graph) => (
                <Card key={graph.id} className="relative">
                  <CardContent className="p-4">
                    <div className="flex items-start justify-between mb-3">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <h4 className="font-medium text-sm truncate">
                            {graph.title}
                          </h4>
                          {getStatusBadge(graph)}
                        </div>
                        {graph.description && (
                          <p className="text-xs text-muted-foreground mb-2 line-clamp-2">
                            {graph.description}
                          </p>
                        )}
                      </div>
                    </div>

                    {}
                    <div className="grid grid-cols-2 gap-4 text-xs text-muted-foreground mb-3">
                      <div className="flex items-center gap-1">
                        <Calendar className="h-3 w-3" />
                        Created: {formatDate(graph.createdAt)}
                      </div>
                      <div className="flex items-center gap-1">
                        <Eye className="h-3 w-3" />
                        Views: {graph.viewCount}
                      </div>
                      {graph.expiresAt && (
                        <div className="flex items-center gap-1">
                          <Calendar className="h-3 w-3" />
                          Expires: {formatDate(graph.expiresAt)}
                        </div>
                      )}
                      <div className="flex items-center gap-1">
                        <Shield className="h-3 w-3" />
                        {graph.isPasswordProtected ? 'Password protected' : 'Open access'}
                      </div>
                    </div>

                    {}
                    <div className="flex flex-wrap gap-1 mb-3">
                      {graph.permissions.allowDownload && (
                        <Badge variant="outline" className="text-xs">Download</Badge>
                      )}
                      {graph.permissions.allowComment && (
                        <Badge variant="outline" className="text-xs">Comments</Badge>
                      )}
                      {graph.permissions.allowEdit && (
                        <Badge variant="outline" className="text-xs">Editing</Badge>
                      )}
                    </div>

                    <Separator className="my-3" />

                    {}
                    <div className="bg-muted/50 rounded p-2 mb-3">
                      <div className="flex items-center justify-between">
                        <div className="text-xs font-mono text-muted-foreground truncate flex-1 mr-2">
                          {graph.shareUrl}
                        </div>
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => handleCopyLink(graph.shareUrl, graph.title)}
                          className="flex-shrink-0"
                        >
                          <Copy className="h-3 w-3" />
                        </Button>
                      </div>
                    </div>

                    {}
                    <div className="flex items-center justify-between">
                      <div className="flex gap-1">
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => window.open(graph.shareUrl, '_blank', 'noopener,noreferrer')}
                        >
                          <ExternalLink className="h-3 w-3 mr-1" />
                          View
                        </Button>
                        {onEdit && (
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => onEdit(graph.id)}
                          >
                            <Edit3 className="h-3 w-3 mr-1" />
                            Edit
                          </Button>
                        )}
                      </div>

                      <Button
                        size="sm"
                        variant="destructive"
                        onClick={() => handleDelete(graph.id, graph.title)}
                        disabled={deletingId === graph.id}
                      >
                        {deletingId === graph.id ? (
                          <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                          <Trash2 className="h-3 w-3" />
                        )}
                      </Button>
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}

          {!isLoading && sharedGraphs.length > 0 && (
            <div className="text-xs text-muted-foreground text-center pt-2 border-t">
              Tip: Expired shares are automatically cleaned up after 30 days
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
};

export default ShareLinkManager;