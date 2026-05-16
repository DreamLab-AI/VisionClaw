// ESLint 9.0.0+ Flat Config for VisionFlow Client
// Handles TypeScript and React with built-in ESLint capabilities
import tseslint from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';

export default [
  // Ignore patterns
  {
    ignores: [
      'node_modules/**',
      'dist/**',
      'build/**',
      '.vite/**',
      'coverage/**',
      '**/*.d.ts',
      'src/types/generated/**',
      'scripts/**',
      '__mocks__/**',
      '.claude-flow/**',
      '.next/**',
    ],
  },

  // CommonJS files
  {
    files: ['**/*.{cjs,mjs}'],
    languageOptions: {
      sourceType: 'module',
      ecmaVersion: 2020,
    },
  },

  // JavaScript files
  {
    files: ['src/**/*.{js,jsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      sourceType: 'module',
      parserOptions: {
        ecmaFeatures: {
          jsx: true,
        },
      },
      globals: {
        React: 'readonly',
        JSX: 'readonly',
      },
    },
    rules: {
      'no-console': [
        'warn',
        {
          allow: ['warn', 'error', 'debug'],
        },
      ],
      'no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      'no-undef': 'warn',
      'no-empty': 'warn',
      'no-trailing-spaces': 'warn',
    },
  },

  // TypeScript and TSX files with @typescript-eslint parser and plugin
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      ecmaVersion: 2020,
      sourceType: 'module',
      parserOptions: {
        ecmaFeatures: {
          jsx: true,
        },
      },
      globals: {
        React: 'readonly',
        JSX: 'readonly',
      },
    },
    plugins: {
      '@typescript-eslint': tseslint,
    },
    rules: {
      // Disable base rules that conflict with TypeScript
      'no-undef': 'off',
      'no-unused-vars': 'off',
      'no-console': [
        'warn',
        {
          allow: ['warn', 'error', 'debug'],
        },
      ],
      'no-empty': 'warn',
      'no-trailing-spaces': 'warn',
      // TypeScript-specific rules
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
    },
  },

  // Phase 6 (ADR-04 D10 / T5): Zero-allocation rendering rule.
  //
  // Banned constructors inside useFrame callbacks: new Float32Array(),
  // new Uint32Array(), new THREE.Vector3(), new THREE.Quaternion(),
  // new THREE.Matrix4(), new THREE.Color(). These are the historical
  // breakage patterns: any per-frame heap allocation in a hot loop creates
  // GC pressure and frame-time spikes.
  //
  // The AST selector targets `useFrame(<callback>)` where callback is an
  // arrow- or function-expression. Any NewExpression matching the banned
  // identifier list inside that callback is reported. The rule is narrow
  // by design — pre-allocated temp objects at module scope satisfy the
  // invariant without false positives.
  //
  // Files under client/src/features/graph/components/ and
  // client/src/features/visualisation/components/ are the primary hot paths.
  {
    files: [
      'src/features/graph/components/**/*.{ts,tsx}',
      'src/features/visualisation/components/**/*.{ts,tsx}',
      'src/rendering/**/*.{ts,tsx}',
    ],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector:
            "CallExpression[callee.name='useFrame'] :matches(ArrowFunctionExpression, FunctionExpression) NewExpression[callee.name=/^(Float32Array|Uint32Array|Uint8Array|Int32Array|Float64Array)$/]",
          message:
            'Zero-alloc rule (ADR-04 D10): no typed-array allocations inside useFrame callbacks. Pre-allocate at module scope.',
        },
        {
          selector:
            "CallExpression[callee.name='useFrame'] :matches(ArrowFunctionExpression, FunctionExpression) NewExpression[callee.object.name='THREE'][callee.property.name=/^(Vector3|Vector2|Vector4|Quaternion|Matrix4|Matrix3|Color|Euler|Frustum|Box3|Sphere)$/]",
          message:
            'Zero-alloc rule (ADR-04 D10): no THREE.* allocations inside useFrame callbacks. Pre-allocate at module scope.',
        },
        {
          selector:
            "CallExpression[callee.name='useFrame'] :matches(ArrowFunctionExpression, FunctionExpression) NewExpression[callee.name=/^(Vector3|Vector2|Vector4|Quaternion|Matrix4|Matrix3|Color|Euler|Frustum)$/]",
          message:
            'Zero-alloc rule (ADR-04 D10): no Vector/Quaternion/Matrix/Color allocations inside useFrame callbacks. Pre-allocate at module scope.',
        },
      ],
    },
  },

  // Test files
  {
    files: ['**/*.{test,spec}.{ts,tsx,js,jsx}', '**/__tests__/**/*.{ts,tsx,js,jsx}'],
    languageOptions: {
      globals: {
        describe: 'readonly',
        it: 'readonly',
        test: 'readonly',
        expect: 'readonly',
        beforeEach: 'readonly',
        afterEach: 'readonly',
        beforeAll: 'readonly',
        afterAll: 'readonly',
        vi: 'readonly',
        jest: 'readonly',
      },
    },
  },
];
