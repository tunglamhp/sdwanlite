import { describe, expect, test, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import Dashboard from './Dashboard';

const mockFetchHealth = vi.fn();

vi.mock('../store', () => ({
  useSdwanStore: () => ({
    token: 'token',
    deviceSummaries: [],
    setDeviceSummaries: vi.fn(),
  }),
}));

vi.mock('../api', () => ({
  fetchHealth: () => mockFetchHealth(),
}));

describe('Dashboard', () => {
  test('renders health and devices sections', async () => {
    mockFetchHealth.mockResolvedValue('ok');

    render(<Dashboard />);

    await waitFor(() => screen.getByText('Health'));
    expect(screen.getByText('Devices')).toBeInTheDocument();
    expect(screen.getByText('ok')).toBeInTheDocument();
  });
});
