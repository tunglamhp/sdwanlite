import { test, expect } from '@playwright/test';

test('dashboard renders', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Devices' })).toBeVisible();
});

test('devices page renders', async ({ page }) => {
  await page.goto('/devices');
  await expect(page.getByRole('heading', { name: 'Devices' })).toBeVisible();
});
