-- The console's vendorable lint script (design/console-surfaces.md
-- "Validation"). The console validates surfaces on load either way; a
-- package that vendors this file into lint/console/surfaces.lua gets the
-- same failures in CI, without a console anywhere near it. Optional by
-- design. The `console` authority is the console's; rule ids stay stable.

function register(lint)
  lint:rule({
    id = "console/surface-shape",
    title = "Surface definition is malformed",
    help = "Give the surface a title, at least one [[bind]], audience values of internal or tenant, and approval as none or role:<id>.",
    target = "catalog=console/surfaces:entry=",
    handler = "check_shape",
  })

  lint:rule({
    id = "console/surface-dangling-binding",
    title = "Surface binds something that does not exist",
    help = "Point every [[bind]] target at a variable, catalog, entry, or layer this package declares.",
    target = "catalog=console/surfaces:entry=",
    handler = "check_bindings",
  })

  lint:rule({
    id = "console/surface-editable-fields",
    title = "Surface edits a field its catalog does not declare",
    help = "Keep editable_fields within the bound catalog's schema properties.",
    target = "catalog=console/surfaces:entry=",
    severity = "warning",
    handler = "check_editable_fields",
  })
end

function surface_binds(entry)
  if type(entry.value) ~= "table" or type(entry.value.bind) ~= "table" then
    return {}
  end
  return entry.value.bind
end

function check_shape(package, entry)
  local value = entry.value
  if type(value) ~= "table" then
    return {
      { message = "surface " .. entry.key .. " must be a table of fields" },
    }
  end
  local diagnostics = {}
  if type(value.title) ~= "string" or value.title == "" then
    table.insert(diagnostics, {
      message = "surface " .. entry.key .. " has no title",
      path = "/value/title",
    })
  end
  if type(value.bind) ~= "table" or #value.bind == 0 then
    table.insert(diagnostics, {
      message = "surface " .. entry.key .. " binds nothing",
      path = "/value/bind",
    })
  end
  if type(value.audience) == "table" then
    for i, audience in ipairs(value.audience) do
      if audience ~= "internal" and audience ~= "tenant" then
        table.insert(diagnostics, {
          message = "surface " .. entry.key .. " audience \"" .. tostring(audience) .. "\" is not internal or tenant",
          path = "/value/audience/" .. (i - 1),
        })
      end
    end
  end
  if value.approval ~= nil then
    if value.approval ~= "none" and not string.match(tostring(value.approval), "^role:[%l%d_]+$") then
      table.insert(diagnostics, {
        message = "surface " .. entry.key .. " approval \"" .. tostring(value.approval) .. "\" is not \"none\" or \"role:<id>\"",
        path = "/value/approval",
      })
    end
  end
  return diagnostics
end

function check_bindings(package, entry)
  local diagnostics = {}
  for i, bind in ipairs(surface_binds(entry)) do
    local pointer = "/value/bind/" .. (i - 1) .. "/target"
    local target = type(bind) == "table" and bind.target or nil
    if type(target) ~= "string" then
      table.insert(diagnostics, {
        message = "surface " .. entry.key .. " bind " .. i .. " has no target",
        path = pointer,
      })
    else
      local catalog_id, entry_key = string.match(target, "^catalog=([%l%d_/]+):entry=(.+)$")
      local variable_id = string.match(target, "^variable=([%l%d_/]+)$")
      local only_catalog = string.match(target, "^catalog=([%l%d_/]+)$")
      local layer_id = string.match(target, "^layer=([%l%d_/]+)$")
      if catalog_id ~= nil then
        local catalog = package.catalogs[catalog_id]
        if catalog == nil then
          table.insert(diagnostics, {
            message = "surface " .. entry.key .. " binds catalog " .. catalog_id .. " which does not exist",
            path = pointer,
          })
        elseif catalog.entries[entry_key] == nil then
          table.insert(diagnostics, {
            message = "surface " .. entry.key .. " binds entry " .. entry_key .. " of catalog " .. catalog_id .. " which does not exist",
            path = pointer,
          })
        end
      elseif variable_id ~= nil then
        if package.variables[variable_id] == nil then
          table.insert(diagnostics, {
            message = "surface " .. entry.key .. " binds variable " .. variable_id .. " which does not exist",
            path = pointer,
          })
        end
      elseif only_catalog ~= nil then
        if package.catalogs[only_catalog] == nil then
          table.insert(diagnostics, {
            message = "surface " .. entry.key .. " binds catalog " .. only_catalog .. " which does not exist",
            path = pointer,
          })
        end
      elseif layer_id ~= nil then
        -- Older rototo versions marshal no layers map; a vendored script
        -- must degrade to silence there, not crash the package's CI.
        local layers = package.layers or {}
        if layers[layer_id] == nil then
          table.insert(diagnostics, {
            message = "surface " .. entry.key .. " binds layer " .. layer_id .. " which does not exist",
            path = pointer,
          })
        end
      else
        table.insert(diagnostics, {
          message = "surface " .. entry.key .. " bind target \"" .. target .. "\" is not a variable, catalog, entry, or layer address",
          path = pointer,
        })
      end
    end
  end
  return diagnostics
end

function check_editable_fields(package, entry)
  local diagnostics = {}
  for i, bind in ipairs(surface_binds(entry)) do
    if type(bind) == "table" and type(bind.editable_fields) == "table" and type(bind.target) == "string" then
      local catalog_id = string.match(bind.target, "^catalog=([%l%d_/]+)")
      local catalog = catalog_id ~= nil and package.catalogs[catalog_id] or nil
      local schema = catalog ~= nil and catalog.json or nil
      local properties = type(schema) == "table" and type(schema.properties) == "table" and schema.properties or nil
      if properties ~= nil then
        for j, field in ipairs(bind.editable_fields) do
          if properties[field] == nil then
            table.insert(diagnostics, {
              message = "surface " .. entry.key .. " editable field \"" .. tostring(field) .. "\" is not declared by catalog " .. catalog_id,
              path = "/value/bind/" .. (i - 1) .. "/editable_fields/" .. (j - 1),
            })
          end
        end
      end
    end
  end
  return diagnostics
end
