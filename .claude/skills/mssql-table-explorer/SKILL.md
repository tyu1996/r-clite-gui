---
name: MSSQL Table Explorer
description: Discover MSSQL table structures, relationships, and cross-database dependencies.
---

# MSSQL Table Explorer

You are an expert MSSQL database analyst specializing in discovering table relationships, dependencies, and schema structures across databases.

## Trigger

Invoke this skill when the user:
- Provides table name(s) and asks to develop a feature
- Asks to explore database relationships for a given table
- Needs to understand table structure and related tables in MSSQL
- Requests analysis of database schema for feature development

## Objectives

1. **Deep Analysis**: Understand the complete structure of the specified table(s)
2. **Width Analysis**: Discover all related tables through various relationship types
3. **Cross-Database Discovery**: Search for related tables across all databases in the same MSSQL instance
4. **Feature Development Context**: Provide comprehensive context for implementing features

## Exploration Strategy

### Phase 1: Table Structure Analysis

For each specified table, gather:
- All columns with data types, nullability, defaults
- Primary keys and unique constraints
- Indexes
- Check constraints
- Computed columns

**SQL Query Template:**
```sql
-- Table columns and details
SELECT
    c.TABLE_CATALOG,
    c.TABLE_SCHEMA,
    c.TABLE_NAME,
    c.COLUMN_NAME,
    c.ORDINAL_POSITION,
    c.DATA_TYPE,
    c.CHARACTER_MAXIMUM_LENGTH,
    c.NUMERIC_PRECISION,
    c.NUMERIC_SCALE,
    c.IS_NULLABLE,
    c.COLUMN_DEFAULT
FROM INFORMATION_SCHEMA.COLUMNS c
WHERE c.TABLE_NAME = '{table_name}'
ORDER BY c.ORDINAL_POSITION;

-- Primary keys and unique constraints
SELECT
    tc.TABLE_NAME,
    tc.CONSTRAINT_NAME,
    tc.CONSTRAINT_TYPE,
    kcu.COLUMN_NAME
FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
    ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
    AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA
WHERE tc.TABLE_NAME = '{table_name}'
    AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE')
ORDER BY kcu.ORDINAL_POSITION;

-- Indexes
SELECT
    t.name AS TableName,
    i.name AS IndexName,
    i.type_desc AS IndexType,
    i.is_unique,
    i.is_primary_key,
    COL_NAME(ic.object_id, ic.column_id) AS ColumnName,
    ic.key_ordinal AS ColumnPosition
FROM sys.tables t
JOIN sys.indexes i ON t.object_id = i.object_id
JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id
WHERE t.name = '{table_name}'
ORDER BY i.name, ic.key_ordinal;
```

### Phase 2: Direct Relationship Discovery

Find tables with direct foreign key relationships:

**SQL Query Template:**
```sql
-- Foreign keys FROM this table (tables this table references)
SELECT
    fk.name AS ForeignKeyName,
    OBJECT_NAME(fk.parent_object_id) AS FromTable,
    COL_NAME(fkc.parent_object_id, fkc.parent_column_id) AS FromColumn,
    OBJECT_NAME(fk.referenced_object_id) AS ToTable,
    COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id) AS ToColumn,
    fk.delete_referential_action_desc AS OnDelete,
    fk.update_referential_action_desc AS OnUpdate
FROM sys.foreign_keys fk
JOIN sys.foreign_key_columns fkc
    ON fk.object_id = fkc.constraint_object_id
WHERE OBJECT_NAME(fk.parent_object_id) = '{table_name}'
ORDER BY fk.name, fkc.constraint_column_id;

-- Foreign keys TO this table (tables that reference this table)
SELECT
    fk.name AS ForeignKeyName,
    OBJECT_NAME(fk.parent_object_id) AS FromTable,
    COL_NAME(fkc.parent_object_id, fkc.parent_column_id) AS FromColumn,
    OBJECT_NAME(fk.referenced_object_id) AS ToTable,
    COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id) AS ToColumn,
    fk.delete_referential_action_desc AS OnDelete,
    fk.update_referential_action_desc AS OnUpdate
FROM sys.foreign_keys fk
JOIN sys.foreign_key_columns fkc
    ON fk.object_id = fkc.constraint_object_id
WHERE OBJECT_NAME(fk.referenced_object_id) = '{table_name}'
ORDER BY fk.name, fkc.constraint_column_id;
```

### Phase 3: Naming Pattern Discovery

Find related tables by naming conventions:

**SQL Query Template:**
```sql
-- Tables with similar naming patterns
SELECT
    TABLE_CATALOG,
    TABLE_SCHEMA,
    TABLE_NAME,
    TABLE_TYPE
FROM INFORMATION_SCHEMA.TABLES
WHERE (
    TABLE_NAME LIKE '{base_name}%' OR
    TABLE_NAME LIKE '%{base_name}%' OR
    TABLE_NAME LIKE '{singular}%' OR
    TABLE_NAME LIKE '{plural}%'
)
AND TABLE_TYPE = 'BASE TABLE'
ORDER BY TABLE_NAME;
```

### Phase 4: Column Name Relationship Discovery

Find tables sharing similar column names (potential implicit relationships):

**SQL Query Template:**
```sql
-- Find tables with matching column names
SELECT DISTINCT
    t1.TABLE_NAME AS RelatedTable,
    t1.COLUMN_NAME AS SharedColumn,
    t1.DATA_TYPE,
    t1.IS_NULLABLE
FROM INFORMATION_SCHEMA.COLUMNS t1
WHERE t1.COLUMN_NAME IN (
    SELECT COLUMN_NAME
    FROM INFORMATION_SCHEMA.COLUMNS
    WHERE TABLE_NAME = '{table_name}'
)
AND t1.TABLE_NAME != '{table_name}'
ORDER BY t1.COLUMN_NAME, t1.TABLE_NAME;

-- Find tables with ID columns that might reference our table
SELECT
    TABLE_NAME,
    COLUMN_NAME,
    DATA_TYPE
FROM INFORMATION_SCHEMA.COLUMNS
WHERE COLUMN_NAME LIKE '%{table_name}%ID'
   OR COLUMN_NAME LIKE '%{table_name}%Id'
   OR COLUMN_NAME LIKE '{table_name}%ID'
   OR COLUMN_NAME LIKE '{table_name}%Id'
ORDER BY TABLE_NAME;
```

### Phase 5: Cross-Database Discovery

Search across all databases in the MSSQL instance:

**SQL Query Template:**
```sql
-- List all databases
SELECT name FROM sys.databases
WHERE database_id > 4  -- Exclude system databases
AND state_desc = 'ONLINE';

-- For each database, execute:
USE [{database_name}];

-- Find tables with similar names
SELECT
    DB_NAME() AS DatabaseName,
    TABLE_SCHEMA,
    TABLE_NAME
FROM INFORMATION_SCHEMA.TABLES
WHERE TABLE_NAME LIKE '%{search_term}%'
    AND TABLE_TYPE = 'BASE TABLE';

-- Find tables with matching columns
SELECT DISTINCT
    DB_NAME() AS DatabaseName,
    TABLE_NAME,
    COLUMN_NAME
FROM INFORMATION_SCHEMA.COLUMNS
WHERE COLUMN_NAME IN ('{column1}', '{column2}', '{column3}')
ORDER BY TABLE_NAME;
```

### Phase 6: Junction/Bridge Table Detection

Identify many-to-many relationship tables:

**SQL Query Template:**
```sql
-- Find potential junction tables (tables with multiple FKs and composite PKs)
SELECT
    t.name AS TableName,
    COUNT(DISTINCT fk.object_id) AS ForeignKeyCount,
    STRING_AGG(OBJECT_NAME(fk.referenced_object_id), ', ') AS ReferencedTables
FROM sys.tables t
LEFT JOIN sys.foreign_keys fk ON t.object_id = fk.parent_object_id
WHERE t.name LIKE '%{table_name}%'
   OR EXISTS (
       SELECT 1 FROM sys.foreign_keys fk2
       WHERE fk2.parent_object_id = t.object_id
       AND fk2.referenced_object_id = OBJECT_ID('{table_name}')
   )
GROUP BY t.name
HAVING COUNT(DISTINCT fk.object_id) >= 2;
```

## Execution Protocol

When this skill is invoked:

1. **Identify the target table(s)** from the user's message
2. **Ask for database connection details** if not already available in the project
3. **Execute Phase 1** to understand the core table structure
4. **Execute Phase 2** to find direct FK relationships
5. **Execute Phase 3-4** to discover implicit relationships
6. **Execute Phase 5** if cross-database search is needed
7. **Execute Phase 6** to identify junction tables
8. **Synthesize findings** into a comprehensive report including:
   - Table structure summary
   - Direct dependencies (tables referenced and referencing)
   - Potential related tables by naming/column patterns
   - Junction tables for many-to-many relationships
   - Recommended tables to include in feature development
   - Data flow diagram (textual representation)

## Output Format

Present findings in this structure:

```
## Database Exploration Results for: {table_name}

### 1. Table Structure
- Columns: [list with types]
- Primary Key: [columns]
- Unique Constraints: [list]
- Indexes: [list]

### 2. Direct Relationships
#### Tables Referenced (Foreign Keys OUT)
- {referenced_table} via {column} -> {ref_column}

#### Tables Referencing (Foreign Keys IN)
- {referencing_table} via {column} <- {this_column}

### 3. Related Tables by Pattern
- [List of tables with similar naming]

### 4. Shared Column Analysis
- [Tables sharing column names with relationship potential]

### 5. Junction Tables
- [Many-to-many bridge tables]

### 6. Cross-Database Findings
- [Related tables in other databases]

### 7. Recommendations for Feature Development
- Core tables to include: [list]
- Optional tables to consider: [list]
- Data flow: [description]
```

## Best Practices

1. **Always ask for confirmation** before executing queries on production databases
2. **Start with read-only queries** using SELECT statements only
3. **Respect the constraint**: NEVER alter database structure or schema
4. **Provide SQL queries** for the user to execute if direct access is not available
5. **Consider performance**: Limit result sets when exploring large databases
6. **Document assumptions**: Note any inferred relationships vs confirmed FK constraints

## Integration with Feature Development

After exploration:
1. Identify the minimal set of tables needed for the feature
2. Map out the data flow and relationships
3. Highlight any potential issues (missing FKs, circular dependencies)
4. Suggest the order of operations for CRUD implementations
5. Note any audit trails, soft deletes, or temporal tables involved

## Example Usage

**User**: "I have an Employees table, help me develop a feature to track employee leaves"

**Your Response**:
1. Invoke this skill to explore Employees table
2. Find related tables: Departments, Positions, LeaveTypes, LeaveRequests, etc.
3. Identify the relationship chain
4. Present complete context for feature development
5. Proceed with implementation using discovered schema
