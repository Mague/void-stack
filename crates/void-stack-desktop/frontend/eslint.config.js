import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
    rules: {
      // eslint-plugin-react-hooks 7.1 also flags the "load data when the
      // project changes" effects (setLoading(true) before the first await),
      // even behind useEffectEvent. That is the pattern the React docs use for
      // fetching; migrating the 16 panels is tracked separately, so it warns
      // instead of failing the build. Everything else stays an error.
      'react-hooks/set-state-in-effect': 'warn',
    },
  },
])
