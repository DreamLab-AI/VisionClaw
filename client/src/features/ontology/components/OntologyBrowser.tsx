import React, { useEffect, useState, useMemo, useCallback } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/features/design-system/components/Card';
import { Input } from '@/features/design-system/components/Input';
import { Button } from '@/features/design-system/components/Button';
import { Badge } from '@/features/design-system/components/Badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/features/design-system/components/Tabs';
import { ScrollArea } from '@/features/design-system/components/ScrollArea';
import {
  Search,
  RefreshCw,
  ChevronRight,
  ChevronDown,
  Layers,
  Link2,
  ExternalLink,
  Copy,
  Clock,
  AlertCircle,
  Wifi,
  WifiOff,
  Info
} from 'lucide-react';
import {
  useOntologyContributionStore,
  OntologyTreeNode,
  OntologyClass,
  OntologyProperty
} from '../hooks/useOntologyStore';

interface OntologyBrowserProps {
  className?: string;
  onNodeSelect?: (iri: string, type: 'class' | 'property') => void;
}

interface TreeNodeProps {
  node: OntologyTreeNode;
  depth: number;
  selectedIri: string | null;
  onToggle: (iri: string) => void;
  onSelect: (iri: string, type: 'class' | 'property') => void;
}

function TreeNode({ node, depth, selectedIri, onToggle, onSelect }: TreeNodeProps) {
  const hasChildren = node.children.length > 0;
  const isSelected = selectedIri === node.iri;

  return (
    <div>
      <div
        className={`flex items-center gap-1 py-1.5 px-2 rounded-md cursor-pointer transition-colors ${
          isSelected
            ? 'bg-primary/10 text-primary'
            : 'hover:bg-muted'
        }`}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={() => {
          if (hasChildren) {
            onToggle(node.iri);
          }
          onSelect(node.iri, node.type);
        }}
      >
        {hasChildren ? (
          <button
            className="p-0.5 hover:bg-muted-foreground/10 rounded"
            onClick={(e) => {
              e.stopPropagation();
              onToggle(node.iri);
            }}
          >
            {node.expanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
          </button>
        ) : (
          <span className="w-5" />
        )}

        {node.type === 'class' ? (
          <Layers className="h-4 w-4 text-blue-500 flex-shrink-0" />
        ) : (
          <Link2 className="h-4 w-4 text-green-500 flex-shrink-0" />
        )}

        <span className="text-sm truncate flex-1">{node.label}</span>

        {node.propertyCount !== undefined && node.propertyCount > 0 && (
          <Badge variant="secondary" className="text-xs h-5">
            {node.propertyCount}
          </Badge>
        )}
      </div>

      {node.expanded && hasChildren && (
        <div>
          {node.children.map((child) => (
            <TreeNode
              key={child.iri}
              node={child}
              depth={depth + 1}
              selectedIri={selectedIri}
              onToggle={onToggle}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function EntityDetails({
  iri,
  type
}: {
  iri: string;
  type: 'class' | 'property';
}) {
  const { getClassByIri, getPropertyByIri } = useOntologyContributionStore();

  const entity = type === 'class'
    ? getClassByIri(iri)
    : getPropertyByIri(iri);

  if (!entity) {
    return (
      <div className="text-center py-8 text-muted-foreground">
        <Info className="h-8 w-8 mx-auto mb-2 opacity-50" />
        <p>Entity not found</p>
      </div>
    );
  }

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
  };

  if (type === 'class') {
    const cls = entity as OntologyClass;
    return (
      <div className="space-y-4">
        <div className="flex items-start justify-between">
          <div>
            <h3 className="font-semibold text-lg">{cls.label}</h3>
            <div className="flex items-center gap-2 mt-1">
              <Badge variant="outline">
                <Layers className="h-3 w-3 mr-1" />
                Class
              </Badge>
            </div>
          </div>
        </div>

        <div className="space-y-3 text-sm">
          <div>
            <div className="text-muted-foreground mb-1">IRI</div>
            <div className="flex items-center gap-2">
              <code className="text-xs bg-muted px-2 py-1 rounded flex-1 break-all">
                {cls.iri}
              </code>
              <Button
                variant="ghost"
                size="sm"
                className="h-8 w-8 p-0"
                onClick={() => copyToClipboard(cls.iri)}
              >
                <Copy className="h-4 w-4" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="h-8 w-8 p-0"
                onClick={() => window.open(cls.iri, '_blank', 'noopener,noreferrer')}
              >
                <ExternalLink className="h-4 w-4" />
              </Button>
            </div>
          </div>

          {cls.parentClass && (
            <div>
              <div className="text-muted-foreground mb-1">Parent Class</div>
              <code className="text-xs bg-muted px-2 py-1 rounded break-all">
                {cls.parentClass}
              </code>
            </div>
          )}

          {cls.description && (
            <div>
              <div className="text-muted-foreground mb-1">Description</div>
              <p className="text-sm">{cls.description}</p>
            </div>
          )}

          {cls.annotations && Object.keys(cls.annotations).length > 0 && (
            <div>
              <div className="text-muted-foreground mb-1">Annotations</div>
              <div className="space-y-1">
                {Object.entries(cls.annotations).map(([key, value]) => (
                  <div key={key} className="flex gap-2 text-xs">
                    <span className="text-muted-foreground">{key}:</span>
                    <span>{value}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    );
  }

  const prop = entity as OntologyProperty;
  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between">
        <div>
          <h3 className="font-semibold text-lg">{prop.label}</h3>
          <div className="flex items-center gap-2 mt-1">
            <Badge variant="outline">
              <Link2 className="h-3 w-3 mr-1" />
              {prop.propertyType} Property
            </Badge>
          </div>
        </div>
      </div>

      <div className="space-y-3 text-sm">
        <div>
          <div className="text-muted-foreground mb-1">IRI</div>
          <div className="flex items-center gap-2">
            <code className="text-xs bg-muted px-2 py-1 rounded flex-1 break-all">
              {prop.iri}
            </code>
            <Button
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0"
              onClick={() => copyToClipboard(prop.iri)}
            >
              <Copy className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {prop.domain && (
          <div>
            <div className="text-muted-foreground mb-1">Domain</div>
            <code className="text-xs bg-muted px-2 py-1 rounded break-all">
              {prop.domain}
            </code>
          </div>
        )}

        {prop.range && (
          <div>
            <div className="text-muted-foreground mb-1">Range</div>
            <code className="text-xs bg-muted px-2 py-1 rounded break-all">
              {prop.range}
            </code>
          </div>
        )}

        {prop.description && (
          <div>
            <div className="text-muted-foreground mb-1">Description</div>
            <p className="text-sm">{prop.description}</p>
          </div>
        )}
      </div>
    </div>
  );
}

export function OntologyBrowser({ className, onNodeSelect }: OntologyBrowserProps) {
  const {
    classTree,
    propertyTree,
    classes,
    properties,
    loading,
    error,
    searchQuery,
    selectedNode,
    subscribed,
    lastUpdate,
    fetchOntology,
    setSearchQuery,
    setSelectedNode,
    toggleNodeExpanded,
    subscribeToUpdates,
    clearError
  } = useOntologyContributionStore();

  const [activeTab, setActiveTab] = useState<'classes' | 'properties'>('classes');
  const [selectedType, setSelectedType] = useState<'class' | 'property'>('class');

  // Subscribe to WebSocket updates on mount
  useEffect(() => {
    const unsubscribe = subscribeToUpdates();
    return unsubscribe;
  }, [subscribeToUpdates]);

  // Fetch ontology on mount if empty
  useEffect(() => {
    if (classes.length === 0 && properties.length === 0 && !loading) {
      fetchOntology();
    }
  }, [classes.length, properties.length, loading, fetchOntology]);

  const handleSearch = useCallback((query: string) => {
    setSearchQuery(query);
  }, [setSearchQuery]);

  const handleNodeSelect = useCallback((iri: string, type: 'class' | 'property') => {
    setSelectedNode(iri);
    setSelectedType(type);
    onNodeSelect?.(iri, type);
  }, [setSelectedNode, onNodeSelect]);

  const handleToggle = useCallback((iri: string) => {
    toggleNodeExpanded(iri);
  }, [toggleNodeExpanded]);

  const handleRefresh = useCallback(() => {
    fetchOntology();
  }, [fetchOntology]);

  // Search results
  const searchResults = useMemo(() => {
    if (!searchQuery.trim()) return null;

    const query = searchQuery.toLowerCase();
    const results: Array<{ type: 'class' | 'property'; item: OntologyClass | OntologyProperty }> = [];

    for (const cls of classes) {
      if (
        cls.label.toLowerCase().includes(query) ||
        cls.iri.toLowerCase().includes(query)
      ) {
        results.push({ type: 'class', item: cls });
      }
    }

    for (const prop of properties) {
      if (
        prop.label.toLowerCase().includes(query) ||
        prop.iri.toLowerCase().includes(query)
      ) {
        results.push({ type: 'property', item: prop });
      }
    }

    return results.slice(0, 50);
  }, [searchQuery, classes, properties]);

  const formatLastUpdate = () => {
    if (!lastUpdate) return 'Never';
    const diff = Date.now() - lastUpdate;
    if (diff < 60000) return 'Just now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    return new Date(lastUpdate).toLocaleTimeString();
  };

  return (
    <Card className={className}>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Search className="h-5 w-5" />
              Browse Ontology
            </CardTitle>
            <CardDescription>
              Explore the public ontology structure
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex items-center gap-1 text-xs text-muted-foreground">
              {subscribed ? (
                <Wifi className="h-3 w-3 text-green-500" />
              ) : (
                <WifiOff className="h-3 w-3 text-red-500" />
              )}
              <Clock className="h-3 w-3" />
              {formatLastUpdate()}
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={handleRefresh}
              disabled={loading}
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </Button>
          </div>
        </div>
      </CardHeader>

      <CardContent className="space-y-4">
        {error && (
          <div className="rounded-lg border border-destructive bg-destructive/10 p-3 flex items-start gap-2">
            <AlertCircle className="h-4 w-4 text-destructive mt-0.5" />
            <div className="flex-1">
              <p className="text-sm text-destructive">{error}</p>
              <Button
                variant="ghost"
                size="sm"
                className="mt-1 h-6 px-2 text-xs"
                onClick={clearError}
              >
                Dismiss
              </Button>
            </div>
          </div>
        )}

        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="Search classes and properties..."
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            className="pl-10"
          />
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
          {/* Tree Browser */}
          <div className="border rounded-lg">
            {searchResults ? (
              <ScrollArea className="h-[400px]">
                <div className="p-2 space-y-1">
                  <div className="text-sm text-muted-foreground mb-2 px-2">
                    {searchResults.length} results for "{searchQuery}"
                  </div>
                  {searchResults.map(({ type, item }) => (
                    <div
                      key={item.iri}
                      className={`flex items-center gap-2 py-2 px-3 rounded-md cursor-pointer transition-colors ${
                        selectedNode === item.iri
                          ? 'bg-primary/10 text-primary'
                          : 'hover:bg-muted'
                      }`}
                      onClick={() => handleNodeSelect(item.iri, type)}
                    >
                      {type === 'class' ? (
                        <Layers className="h-4 w-4 text-blue-500" />
                      ) : (
                        <Link2 className="h-4 w-4 text-green-500" />
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-medium truncate">{item.label}</div>
                        <div className="text-xs text-muted-foreground truncate">{item.iri}</div>
                      </div>
                    </div>
                  ))}
                  {searchResults.length === 0 && (
                    <div className="text-center py-8 text-muted-foreground">
                      No results found
                    </div>
                  )}
                </div>
              </ScrollArea>
            ) : (
              <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as 'classes' | 'properties')}>
                <div className="border-b px-2">
                  <TabsList className="w-full grid grid-cols-2 h-10">
                    <TabsTrigger value="classes" className="text-sm">
                      <Layers className="h-4 w-4 mr-1" />
                      Classes ({classes.length})
                    </TabsTrigger>
                    <TabsTrigger value="properties" className="text-sm">
                      <Link2 className="h-4 w-4 mr-1" />
                      Properties ({properties.length})
                    </TabsTrigger>
                  </TabsList>
                </div>

                <TabsContent value="classes" className="m-0">
                  <ScrollArea className="h-[360px]">
                    <div className="p-2">
                      {classTree.length === 0 && !loading ? (
                        <div className="text-center py-8 text-muted-foreground">
                          No classes loaded
                        </div>
                      ) : (
                        classTree.map((node) => (
                          <TreeNode
                            key={node.iri}
                            node={node}
                            depth={0}
                            selectedIri={selectedNode}
                            onToggle={handleToggle}
                            onSelect={handleNodeSelect}
                          />
                        ))
                      )}
                    </div>
                  </ScrollArea>
                </TabsContent>

                <TabsContent value="properties" className="m-0">
                  <ScrollArea className="h-[360px]">
                    <div className="p-2">
                      {propertyTree.length === 0 && !loading ? (
                        <div className="text-center py-8 text-muted-foreground">
                          No properties loaded
                        </div>
                      ) : (
                        propertyTree.map((node) => (
                          <TreeNode
                            key={node.iri}
                            node={node}
                            depth={0}
                            selectedIri={selectedNode}
                            onToggle={handleToggle}
                            onSelect={handleNodeSelect}
                          />
                        ))
                      )}
                    </div>
                  </ScrollArea>
                </TabsContent>
              </Tabs>
            )}
          </div>

          {/* Details Panel */}
          <div className="border rounded-lg p-4">
            {selectedNode ? (
              <EntityDetails iri={selectedNode} type={selectedType} />
            ) : (
              <div className="h-full flex items-center justify-center text-muted-foreground">
                <div className="text-center">
                  <Info className="h-12 w-12 mx-auto mb-3 opacity-50" />
                  <p className="font-medium">Select an entity</p>
                  <p className="text-sm mt-1">
                    Click on a class or property to view details
                  </p>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Statistics */}
        <div className="flex items-center gap-4 text-sm text-muted-foreground pt-2 border-t">
          <div className="flex items-center gap-1">
            <Layers className="h-4 w-4" />
            {classes.length} classes
          </div>
          <div className="flex items-center gap-1">
            <Link2 className="h-4 w-4" />
            {properties.length} properties
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
