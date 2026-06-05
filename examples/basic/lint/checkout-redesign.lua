function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value.heading",
    rule = {
          id = "consumer-experience/checkout-heading-required",
          title = "Checkout heading is missing",
          help = "Set heading to visible checkout copy.",
        },
    handler = "check_heading",
  })

  lint:on({
    stage = "value",
    entity = "value",
    field = "value.image_url",
    rule = {
          id = "consumer-experience/checkout-image-path",
          title = "Checkout image path is invalid",
          help = "Use an image URL under /images/checkout/.",
        },
    handler = "check_image_path",
  })
end

function is_checkout_value(value)
  return type(value) == "table" and value.variant ~= nil and value.image_url ~= nil
end

function check_heading(ctx)
  if is_checkout_value(ctx.target.value) and ctx.target.value.heading == "" then
    return {
      {
        message = "checkout value " .. ctx.target.name .. " must include heading"
      }
    }
  end
  return {}
end

function check_image_path(ctx)
  if is_checkout_value(ctx.target.value)
      and not string.match(ctx.target.value.image_url, "^/images/checkout/") then
    return {
      {
        message = "checkout value " .. ctx.target.name .. " must use a checkout image path"
      }
    }
  end
  return {}
end
