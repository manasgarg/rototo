function register(lint)
  lint:rule({
    id = "targets/package-extends",
    title = "Package extends target was checked",
    help = "Update the package extends policy.",
    target = "/",
    handler = "check_package",
  })

  lint:rule({
    id = "targets/variable-type",
    title = "Variable type target was checked",
    help = "Update the variable type policy.",
    target = "/variables/agent_config",
    handler = "check_variable",
  })

  lint:rule({
    id = "targets/returned-variable-type",
    title = "Returned variable type field was checked",
    help = "Update the returned field policy.",
    target = "/variables/agent_config",
    handler = "check_returned_variable_field",
  })

  lint:rule({
    id = "targets/invalid-returned-field",
    title = "Invalid returned field fell back",
    help = "Update the invalid returned field policy.",
    target = "/variables/agent_config",
    handler = "check_invalid_returned_field",
  })

  lint:rule({
    id = "targets/package-variable-default",
    title = "Package target can point at a variable field",
    help = "Update the package variable pointer policy.",
    target = "/",
    handler = "check_package_variable_default",
  })

  lint:rule({
    id = "targets/catalog-entry-json-pointer",
    title = "Catalog entry target can point at value JSON",
    help = "Update the catalog entry pointer policy.",
    target = "/catalogs/agent_config/entries/standard",
    handler = "check_catalog_entry_value",
  })
end

function contains_location(value)
  if type(value) ~= "table" then
    return false
  end

  if value.location ~= nil then
    return true
  end

  for _, child in pairs(value) do
    if contains_location(child) then
      return true
    end
  end

  return false
end

function check_package(package, target)
  return {
    {
      message = "package target checked extends",
      path = "/manifest/extends",
    },
  }
end

function check_variable(package, variable)
  if variable.declaration.kind == "catalog" and
      not contains_location(package) and
      not contains_location(variable) then
    return {
      {
        message = "variable target checked type",
        path = "/declaration/value",
      },
    }
  end
  return {}
end

function check_returned_variable_field(package, variable)
  if variable.declaration.kind == "catalog" then
    return {
      {
        message = "variable target checked returned type field",
        path = "/declaration/value",
      },
    }
  end
  return {}
end

function check_invalid_returned_field(package, variable)
  if variable.id == "agent_config" then
    return {
      {
        message = "variable target fell back for invalid returned field",
        path = "missing..field",
      },
    }
  end
  return {}
end

function check_package_variable_default(package, target)
  return {
    {
      message = "package target checked variable default",
      path = "/variables/agent_config/resolve/default",
    },
  }
end

function check_catalog_entry_value(package, entry)
  if entry.value.max_output_tokens == 1000 then
    return {
      {
        message = "catalog entry target checked nested value",
        path = "/value/max_output_tokens",
      },
    }
  end
  return {}
end
