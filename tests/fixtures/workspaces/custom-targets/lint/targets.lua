function register(lint)
  lint:on({
    stage = "project",
    entity = "workspace",
    field = "extends",
    rule = {
          id = "targets/workspace-extends",
          title = "Workspace extends target was checked",
          help = "Update the workspace extends policy.",
        },
    handler = "check_workspace",
  })

  lint:on({
    stage = "project",
    entity = "qualifier",
    field = "predicates",
    rule = {
          id = "targets/qualifier-predicates",
          title = "Qualifier predicates target was checked",
          help = "Update the qualifier predicate policy.",
        },
    handler = "check_qualifier",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    field = "type",
    rule = {
          id = "targets/variable-type",
          title = "Variable type target was checked",
          help = "Update the variable type policy.",
        },
    handler = "check_variable",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    rule = {
          id = "targets/returned-variable-type",
          title = "Returned variable type field was checked",
          help = "Update the returned field policy.",
        },
    handler = "check_returned_variable_field",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    rule = {
          id = "targets/invalid-returned-field",
          title = "Invalid returned field fell back",
          help = "Update the invalid returned field policy.",
        },
    handler = "check_invalid_returned_field",
  })

  lint:on({
    stage = "value",
    entity = "schema",
    field = "json.properties",
    rule = {
          id = "targets/schema-json",
          title = "Schema JSON target was checked",
          help = "Update the schema JSON policy.",
        },
    handler = "check_schema",
  })
end

function check_workspace(ctx)
  return {
    { message = "workspace target checked extends" },
  }
end

function check_qualifier(ctx)
  if ctx.target.id == "premium-users" then
    return {
      { message = "qualifier target checked predicates" },
    }
  end
  return {}
end

function check_variable(ctx)
  if ctx.target.toml.type ~= nil then
    return {
      { message = "variable target checked type" },
    }
  end
  return {}
end

function check_returned_variable_field(ctx)
  if ctx.target.toml.type ~= nil then
    return {
      {
        message = "variable target checked returned type field",
        field = "type",
      },
    }
  end
  return {}
end

function check_invalid_returned_field(ctx)
  if ctx.target.id == "agent-config" then
    return {
      {
        message = "variable target fell back for invalid returned field",
        field = "missing..field",
      },
    }
  end
  return {}
end

function check_schema(ctx)
  if ctx.target.selected.max_output_tokens ~= nil then
    return {
      { message = "schema target checked JSON properties" },
    }
  end
  return {}
end
