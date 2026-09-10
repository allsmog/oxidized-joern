signed char identity(signed char value) { signed char local = value; return local; }
signed char declared(signed char value);
signed char call_definition(signed char value) { return identity(value); }
signed char call_prototype(signed char value) { return declared(value); }
