/**
 * PodSettings Component
 * Pod configuration panel for control panel integration
 */

import React, { useState, useCallback } from 'react';
import { Database, Copy, Check, User, HardDrive, Trash2, Plus, RefreshCw, ExternalLink, AlertTriangle } from 'lucide-react';
import { Button } from '@/features/design-system/components/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@/features/design-system/components/Card';
import { Badge } from '@/features/design-system/components/Badge';
import { Separator } from '@/features/design-system/components/Separator';
import { Input } from '@/features/design-system/components/Input';
import { Label } from '@/features/design-system/components/Label';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogDescription,
} from '@/features/design-system/components/Dialog';
import {
  TooltipProvider,
  TooltipRoot,
  TooltipTrigger,
  TooltipContent,
} from '@/features/design-system/components/Tooltip';
import { cn } from '@/utils/classNameUtils';
import { useSolidPod } from '../hooks/useSolidPod';

interface CopyButtonProps {
  text: string;
  className?: string;
}

const CopyButton: React.FC<CopyButtonProps> = ({ text, className }) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <TooltipProvider>
      <TooltipRoot>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={handleCopy}
            className={cn('h-6 w-6', className)}
          >
            {copied ? (
              <Check className="h-3 w-3 text-green-500" />
            ) : (
              <Copy className="h-3 w-3" />
            )}
          </Button>
        </TooltipTrigger>
        <TooltipContent>{copied ? 'Copied!' : 'Copy'}</TooltipContent>
      </TooltipRoot>
    </TooltipProvider>
  );
};

interface InfoRowProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  copyable?: boolean;
}

const InfoRow: React.FC<InfoRowProps> = ({ icon, label, value, copyable }) => {
  return (
    <div className="flex items-start gap-3 py-2">
      <div className="mt-0.5 text-muted-foreground">{icon}</div>
      <div className="flex-1 min-w-0">
        <div className="text-xs text-muted-foreground mb-0.5">{label}</div>
        <div className="flex items-center gap-2">
          <span className="text-sm font-mono truncate">{value}</span>
          {copyable && <CopyButton text={value} />}
        </div>
      </div>
    </div>
  );
};

export interface PodSettingsProps {
  className?: string;
}

export const PodSettings: React.FC<PodSettingsProps> = ({ className }) => {
  const {
    podInfo,
    isLoading,
    error,
    checkPod,
    createPod,
    deletePod,
  } = useSolidPod();

  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const [podName, setPodName] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);

  // Storage quota not available via Solid Pod API — omit fake data
  const storageAvailable = false;

  const handleCreatePod = useCallback(async () => {
    setIsCreating(true);
    try {
      const result = await createPod(podName || undefined);
      if (result.success) {
        setShowCreateDialog(false);
        setPodName('');
      }
    } finally {
      setIsCreating(false);
    }
  }, [createPod, podName]);

  const handleDeletePod = useCallback(async () => {
    setIsDeleting(true);
    try {
      const success = await deletePod();
      if (success) {
        setShowDeleteDialog(false);
      }
    } finally {
      setIsDeleting(false);
    }
  }, [deletePod]);

  if (isLoading) {
    return (
      <Card className={cn('w-full', className)}>
        <CardContent className="py-8 flex items-center justify-center">
          <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground" />
        </CardContent>
      </Card>
    );
  }

  if (!podInfo?.exists) {
    return (
      <Card className={cn('w-full', className)}>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Database className="h-4 w-4" />
            Solid Pod
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col items-center justify-center py-6 text-center">
            <Database className="h-12 w-12 text-muted-foreground mb-3" />
            <p className="text-sm text-muted-foreground mb-1">
              No Solid Pod found
            </p>
            <p className="text-xs text-muted-foreground mb-4">
              Create a pod to store your data securely
            </p>
            <Button onClick={() => setShowCreateDialog(true)}>
              <Plus className="h-4 w-4 mr-2" />
              Create Pod
            </Button>
          </div>

          {error && (
            <div className="p-3 rounded-md bg-destructive/10 text-destructive text-sm flex items-center gap-2">
              <AlertTriangle className="h-4 w-4 flex-shrink-0" />
              {error}
            </div>
          )}
        </CardContent>

        {/* Create Pod Dialog */}
        <Dialog open={showCreateDialog} onOpenChange={setShowCreateDialog}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Create Solid Pod</DialogTitle>
              <DialogDescription>
                A pod stores your personal data. You control who has access.
              </DialogDescription>
            </DialogHeader>

            <div className="space-y-4 py-4">
              <div className="space-y-2">
                <Label htmlFor="podName">Pod Name (optional)</Label>
                <Input
                  id="podName"
                  placeholder="my-pod"
                  value={podName}
                  onChange={(e) => setPodName(e.target.value)}
                />
                <p className="text-xs text-muted-foreground">
                  Leave empty for a default name based on your identity
                </p>
              </div>
            </div>

            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => setShowCreateDialog(false)}
              >
                Cancel
              </Button>
              <Button onClick={handleCreatePod} loading={isCreating}>
                Create Pod
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </Card>
    );
  }

  return (
    <Card className={cn('w-full', className)}>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Database className="h-4 w-4" />
            Solid Pod
          </CardTitle>
          <Badge variant="success" className="text-xs">
            Active
          </Badge>
        </div>
      </CardHeader>

      <CardContent className="space-y-4">
        {/* Pod URL */}
        <InfoRow
          icon={<Database className="h-4 w-4" />}
          label="Pod URL"
          value={podInfo.podUrl || 'Unknown'}
          copyable
        />

        {/* WebID */}
        <InfoRow
          icon={<User className="h-4 w-4" />}
          label="WebID"
          value={podInfo.webId || 'Unknown'}
          copyable
        />

        <Separator />

        {/* Storage Quota — only render when real data is available */}
        {storageAvailable && (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-sm">
              <div className="flex items-center gap-2 text-muted-foreground">
                <HardDrive className="h-4 w-4" />
                Storage
              </div>
              <span className="text-xs text-muted-foreground">Not available</span>
            </div>
          </div>
        )}

        <Separator />

        {/* Actions */}
        <div className="flex items-center justify-between">
          <TooltipProvider>
            <TooltipRoot>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="sm" onClick={checkPod}>
                  <RefreshCw className="h-4 w-4 mr-2" />
                  Refresh
                </Button>
              </TooltipTrigger>
              <TooltipContent>Refresh pod status</TooltipContent>
            </TooltipRoot>
          </TooltipProvider>

          {podInfo.podUrl && (
            <TooltipProvider>
              <TooltipRoot>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => window.open(podInfo.podUrl, '_blank', 'noopener,noreferrer')}
                  >
                    <ExternalLink className="h-4 w-4 mr-2" />
                    Open
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Open pod in new tab</TooltipContent>
              </TooltipRoot>
            </TooltipProvider>
          )}

          <TooltipProvider>
            <TooltipRoot>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  onClick={() => setShowDeleteDialog(true)}
                >
                  <Trash2 className="h-4 w-4 mr-2" />
                  Delete
                </Button>
              </TooltipTrigger>
              <TooltipContent>Delete pod</TooltipContent>
            </TooltipRoot>
          </TooltipProvider>
        </div>

        {error && (
          <div className="p-3 rounded-md bg-destructive/10 text-destructive text-sm flex items-center gap-2">
            <AlertTriangle className="h-4 w-4 flex-shrink-0" />
            {error}
          </div>
        )}
      </CardContent>

      {/* Delete Pod Dialog */}
      <Dialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2 text-destructive">
              <AlertTriangle className="h-5 w-5" />
              Delete Pod
            </DialogTitle>
            <DialogDescription>
              This action cannot be undone. All data stored in your pod will be
              permanently deleted.
            </DialogDescription>
          </DialogHeader>

          <div className="py-4">
            <div className="p-4 rounded-md bg-destructive/10 border border-destructive/20">
              <p className="text-sm font-medium">This will delete:</p>
              <ul className="mt-2 text-sm text-muted-foreground list-disc list-inside">
                <li>All stored files and resources</li>
                <li>Your WebID profile</li>
                <li>All access permissions</li>
              </ul>
            </div>
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setShowDeleteDialog(false)}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={handleDeletePod}
              loading={isDeleting}
            >
              Delete Pod
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
};

export default PodSettings;
