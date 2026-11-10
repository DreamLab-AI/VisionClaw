import { test, expect } from '@playwright/test';

test.describe('Quality Gates Feature', () => {

  test('Quality Gates API returns all expected settings', async ({ request }) => {
    const response = await request.get('/api/settings/quality-gates');
    expect(response.ok()).toBe(true);

    const data = await response.json();
    console.log('Quality Gates API response:', JSON.stringify(data, null, 2));

    // Verify all expected fields exist
    expect(data).toHaveProperty('gpuAcceleration');
    expect(data).toHaveProperty('ontologyReasoning');
    expect(data).toHaveProperty('semanticForces');
    expect(data).toHaveProperty('layoutMode');
    expect(data).toHaveProperty('showClusters');
    expect(data).toHaveProperty('showAnomalies');
    expect(data).toHaveProperty('showCommunities');
    expect(data).toHaveProperty('ruvectorEnabled');
    expect(data).toHaveProperty('gnnPhysics');
    expect(data).toHaveProperty('minFpsThreshold');
    expect(data).toHaveProperty('maxNodeCount');
    expect(data).toHaveProperty('autoAdjust');

    // Verify default values
    expect(data.gpuAcceleration).toBe(true);
    expect(data.layoutMode).toBe('force-directed');
    expect(data.minFpsThreshold).toBe(30);
    expect(data.maxNodeCount).toBe(10000);
  });

  test('Quality Gates API can update settings', async ({ request }) => {
    // Update a setting
    const updateResponse = await request.put('/api/settings/quality-gates', {
      data: {
        gpuAcceleration: false,
        ontologyReasoning: true,
        semanticForces: false,
        layoutMode: 'dag-topdown',
        showClusters: true,
        showAnomalies: true,
        showCommunities: false,
        ruvectorEnabled: false,
        gnnPhysics: false,
        minFpsThreshold: 45,
        maxNodeCount: 15000,
        autoAdjust: false
      }
    });
    expect(updateResponse.ok()).toBe(true);

    // Verify the update
    const getResponse = await request.get('/api/settings/quality-gates');
    const data = await getResponse.json();

    console.log('Updated Quality Gates:', JSON.stringify(data, null, 2));

    expect(data.gpuAcceleration).toBe(false);
    expect(data.ontologyReasoning).toBe(true);
    expect(data.layoutMode).toBe('dag-topdown');
    expect(data.minFpsThreshold).toBe(45);
    expect(data.maxNodeCount).toBe(15000);
    expect(data.autoAdjust).toBe(false);

    // Restore default values
    await request.put('/api/settings/quality-gates', {
      data: {
        gpuAcceleration: true,
        ontologyReasoning: false,
        semanticForces: false,
        layoutMode: 'force-directed',
        showClusters: true,
        showAnomalies: true,
        showCommunities: false,
        ruvectorEnabled: false,
        gnnPhysics: false,
        minFpsThreshold: 30,
        maxNodeCount: 10000,
        autoAdjust: true
      }
    });
  });

  test('VisionClaw Control Center renders correctly', async ({ page }) => {
    await page.goto('http://localhost:3002');
    await page.waitForTimeout(5000);

    // Take screenshot
    await page.screenshot({ path: 'test-results/control-center.png', fullPage: true });

    // Check page title
    const title = await page.title();
    console.log('Page title:', title);
    expect(title).toContain('VisionClaw');

    // Verify Control Center exists
    const pageContent = await page.content();
    expect(pageContent).toContain('Control Center');
    expect(pageContent).toContain('quality-gates');

    console.log('Control Center rendered with Quality Gates tab');
  });
});
