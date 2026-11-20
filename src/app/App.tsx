import { AppProviders } from "./providers/app-providers";
import { AppRoutes } from "./router";

export function App() {
  return (
    <AppProviders>
      <AppRoutes />
    </AppProviders>
  );
}
