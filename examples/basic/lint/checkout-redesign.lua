function lint(variable)
  local diagnostics = {}
  local values = variable.toml.variable.values

  for name, value in pairs(values) do
    if value.heading == "" then
      table.insert(diagnostics, {
        message = "checkout value " .. name .. " must include heading",
        help = "Set heading to visible checkout copy."
      })
    end

    if not string.match(value.image_url, "^/images/checkout/") then
      table.insert(diagnostics, {
        message = "checkout value " .. name .. " must use a checkout image path",
        help = "Use an image URL under /images/checkout/."
      })
    end
  end

  return diagnostics
end
