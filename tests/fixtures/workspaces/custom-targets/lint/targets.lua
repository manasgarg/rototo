function register(lint)
  lint:on({
    stage = "project",
    entity = "workspace",
    field = "environments",
    rule = "targets/workspace-environments",
    handler = "check_workspace",
  })

  lint:on({
    stage = "project",
    entity = "qualifier",
    field = "predicates",
    rule = "targets/qualifier-predicates",
    handler = "check_qualifier",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    field = "type",
    rule = "targets/variable-type",
    handler = "check_variable",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    rule = "targets/returned-variable-type",
    handler = "check_returned_variable_field",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    rule = "targets/invalid-returned-field",
    handler = "check_invalid_returned_field",
  })

  lint:on({
    stage = "value",
    entity = "schema",
    field = "json.properties",
    rule = "targets/schema-json",
    handler = "check_schema",
  })
end

function check_workspace(ctx)
  if ctx.target.environments[1] == "prod" then
    return {
      { message = "workspace target checked prod environment" },
    }
  end
  return {}
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
