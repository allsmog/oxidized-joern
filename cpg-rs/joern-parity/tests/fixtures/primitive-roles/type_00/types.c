char identity(char value) { char local = value; return local; }
char declared(char value);
char call_definition(char value) { return identity(value); }
char call_prototype(char value) { return declared(value); }
