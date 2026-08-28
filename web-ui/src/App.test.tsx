import { render, screen } from '@testing-library/react';
import App from './App';

test('renders sidebar navigation labels', () => {
  render(<App />);

  expect(screen.getByRole('link', { name: 'Dashboard' })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Devices' })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Topology' })).toBeInTheDocument();
});
