import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: ['dist/**', 'node_modules/**', 'src-tauri/target/**', 'extensions/**'],
  },
  {
    files: ['**/*.ts', '**/*.tsx'],
    extends: [tseslint.configs.recommended],
    rules: {
      'curly': 'warn',
      'eqeqeq': 'warn',
      'prefer-const': ['warn', { destructuring: 'all' }],
      'no-caller': 'warn',
      'no-debugger': 'warn',
      'no-duplicate-imports': 'warn',
      'no-eval': 'warn',
      'no-new-wrappers': 'warn',
      'no-throw-literal': 'warn',
      'no-var': 'warn',
      'no-restricted-globals': ['warn', 'name', 'length', 'event', 'closed', 'status', 'origin'],
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': ['warn', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
      '@typescript-eslint/naming-convention': [
        'warn',
        { selector: 'class', format: ['PascalCase'] },
        { selector: 'interface', format: ['PascalCase'], prefix: ['I'] },
      ],
    },
  },
  {
    files: ['**/*.cjs'],
    rules: {
      'curly': 'warn',
      'eqeqeq': 'warn',
      'no-caller': 'warn',
      'no-debugger': 'warn',
      'no-eval': 'warn',
      'no-var': 'warn',
    },
  },
);
